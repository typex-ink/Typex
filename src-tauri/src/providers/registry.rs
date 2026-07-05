//! ProviderRegistry：由配置构造 provider 实例（03 §5 / 07 §5.1）。
//!
//! - 按 profile-id 惰性构建 + 缓存；settings 变更时失效重建
//! - 密钥经 SecretStore 解析（keyring:// 引用 → 明文只在内存）

use crate::error::{ErrorCode, Result, TypexError};
use crate::providers::http;
use crate::providers::llm::{
    LlmProvider, chat_completions::ChatCompletionsLlm, responses::ResponsesLlm,
};
use crate::providers::stt::{
    SttProvider, openai_compat::OpenAiCompatStt, volcengine::VolcengineStt,
};
use crate::settings::schema::Settings;
use crate::settings::secrets::SecretStore;
use crate::types::{ProviderKind, ProviderProfile, SlotKind};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct ProviderRegistry {
    secrets: Arc<dyn SecretStore>,
    /// profile-id → 已构建实例
    stt_cache: Mutex<HashMap<String, Arc<dyn SttProvider>>>,
    llm_cache: Mutex<HashMap<String, Arc<dyn LlmProvider>>>,
    /// 当前配置快照（settings watch 更新）
    settings: Mutex<Settings>,
    /// 本地模型存储根（app_data_dir；v1.1 local-models）
    #[cfg_attr(not(feature = "local-models"), allow(dead_code))]
    models_data_dir: Mutex<Option<std::path::PathBuf>>,
}

impl ProviderRegistry {
    pub fn new(settings: Settings, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            secrets,
            stt_cache: Mutex::new(HashMap::new()),
            llm_cache: Mutex::new(HashMap::new()),
            settings: Mutex::new(settings),
            models_data_dir: Mutex::new(None),
        }
    }

    /// 设定本地模型存储根（runner 启动时注入 app_data_dir；v1.1）。
    pub fn set_models_data_dir(&self, dir: std::path::PathBuf) {
        *self.models_data_dir.lock().unwrap() = Some(dir);
    }

    /// 配置变更：只清缓存中已消失/已变更的 profile（07 §5.1 惰性重建）。
    pub fn on_settings_changed(&self, new: Settings) {
        let old = self.settings.lock().unwrap().clone();
        let changed: Vec<String> = old
            .profiles
            .iter()
            .filter(|op| {
                new.profiles
                    .iter()
                    .find(|np| np.id == op.id)
                    .is_none_or(|np| np != *op)
            })
            .map(|p| p.id.clone())
            .collect();
        {
            let mut stt = self.stt_cache.lock().unwrap();
            let mut llm = self.llm_cache.lock().unwrap();
            for id in &changed {
                stt.remove(id);
                llm.remove(id);
            }
        }
        *self.settings.lock().unwrap() = new;
    }

    fn profile_for_slot(&self, slot: SlotKind) -> Result<ProviderProfile> {
        let s = self.settings.lock().unwrap();
        let active = s.slots.get(&slot).and_then(|c| c.active_profile.clone());
        let Some(active) = active else {
            drop(s);
            // 零配置兜底（ADR-20）：STT/整理/翻译三槽未配置时指向本地档案
            // （模型已下载前提）；问答槽无兜底。
            return self.local_fallback_profile(slot);
        };
        s.profiles
            .iter()
            .find(|p| p.id == active)
            .cloned()
            .ok_or_else(|| {
                TypexError::new(ErrorCode::NotConfigured, format!("档案 {active} 不存在"))
            })
    }

    /// ADR-20 零配置兜底：合成本地档案（不落盘）。
    #[cfg(feature = "local-models")]
    fn local_fallback_profile(&self, slot: SlotKind) -> Result<ProviderProfile> {
        use crate::local::{download, manifest};
        if slot == SlotKind::Assistant {
            return Err(TypexError::new(
                ErrorCode::NotConfigured,
                "Assistant 槽未配置（问答槽无本地兜底，ADR-20）",
            ));
        }
        let dir = self
            .models_data_dir
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| {
                TypexError::new(ErrorCode::NotConfigured, format!("{slot:?} 槽未配置"))
            })?;
        let catalog = manifest::catalog();
        let downloaded = download::list_downloaded(&dir, &catalog);
        let want = if slot == SlotKind::Stt {
            manifest::ModelPurpose::Stt
        } else {
            manifest::ModelPurpose::Llm
        };
        // 清单序 = 档位从轻到重；取已下载中最靠前（最轻）者
        let model = catalog
            .into_iter()
            .find(|m| m.purpose == want && downloaded.contains(&m.id))
            .ok_or_else(|| {
                TypexError::new(
                    ErrorCode::NotConfigured,
                    format!("{slot:?} 槽未配置（本地模型未下载）"),
                )
            })?;
        Ok(ProviderProfile {
            id: format!("local-{}", model.id),
            slots: vec![slot],
            kind: ProviderKind::Local,
            label: format!("本地 · {}", model.display_name),
            base_url: String::new(),
            model: model.id,
            credentials: HashMap::new(),
            extra_headers: HashMap::new(),
            extra_form: HashMap::new(),
            timeout_ms: 120_000,
            options: HashMap::new(),
        })
    }

    #[cfg(not(feature = "local-models"))]
    fn local_fallback_profile(&self, slot: SlotKind) -> Result<ProviderProfile> {
        Err(TypexError::new(
            ErrorCode::NotConfigured,
            format!("{slot:?} 槽未配置"),
        ))
    }

    fn resolve_secret(&self, profile: &ProviderProfile, field: &str) -> Result<String> {
        let reference = profile.credentials.get(field).ok_or_else(|| {
            TypexError::new(
                ErrorCode::NotConfigured,
                format!("档案 {} 缺少凭据 {field}", profile.id),
            )
        })?;
        self.secrets.get(reference)
    }

    fn http_client(&self, timeout_ms: u64) -> reqwest::Client {
        let s = self.settings.lock().unwrap();
        http::build_client(s.general.proxy_mode, &s.general.proxy_url, timeout_ms)
    }

    /// 取某槽位的 STT provider。
    pub fn stt_for(&self, slot: SlotKind) -> Result<Arc<dyn SttProvider>> {
        let profile = self.profile_for_slot(slot)?;
        if let Some(p) = self.stt_cache.lock().unwrap().get(&profile.id) {
            return Ok(p.clone());
        }
        let provider = self.build_stt(&profile)?;
        self.stt_cache
            .lock()
            .unwrap()
            .insert(profile.id.clone(), provider.clone());
        Ok(provider)
    }

    /// 取某槽位的 LLM provider。
    pub fn llm_for(&self, slot: SlotKind) -> Result<Arc<dyn LlmProvider>> {
        let profile = self.profile_for_slot(slot)?;
        if let Some(p) = self.llm_cache.lock().unwrap().get(&profile.id) {
            return Ok(p.clone());
        }
        let provider = self.build_llm(&profile)?;
        self.llm_cache
            .lock()
            .unwrap()
            .insert(profile.id.clone(), provider.clone());
        Ok(provider)
    }

    /// 由 profile 直接构建（测试连接用——不走 slot 指针）。
    pub fn build_stt(&self, profile: &ProviderProfile) -> Result<Arc<dyn SttProvider>> {
        match profile.kind {
            ProviderKind::OpenaiCompat => {
                let key = self.resolve_secret(profile, "api_key")?;
                let client = self.http_client(profile.timeout_ms);
                Ok(Arc::new(
                    OpenAiCompatStt::new(
                        client,
                        profile.base_url.clone(),
                        key,
                        profile.model.clone(),
                    )
                    .with_extras(profile.extra_headers.clone(), profile.extra_form.clone()),
                ))
            }
            ProviderKind::Volcengine => {
                let app_key = self.resolve_secret(profile, "app_key")?;
                let access_key = self.resolve_secret(profile, "access_token")?;
                let client = self.http_client(profile.timeout_ms);
                let mut p =
                    VolcengineStt::new(client, profile.base_url.clone(), app_key, access_key);
                if let Some(rid) = profile.options.get("resource_id").and_then(|v| v.as_str()) {
                    p = p.with_resource_id(rid);
                }
                Ok(Arc::new(p))
            }
            #[cfg(feature = "local-models")]
            ProviderKind::Local => self.build_local_stt(profile),
            _ => Err(TypexError::new(
                ErrorCode::InvalidRequest,
                format!("{:?} 不是 STT 类型", profile.kind),
            )),
        }
    }

    /// 本地 STT（v1.1）：按 model id 选引擎（sherpa / llama mtmd）。
    #[cfg(feature = "local-models")]
    fn build_local_stt(&self, profile: &ProviderProfile) -> Result<Arc<dyn SttProvider>> {
        use crate::local::{manifest, stt_qwen_asr, stt_sense_voice};
        let dir = self
            .models_data_dir
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| TypexError::new(ErrorCode::NotConfigured, "本地模型目录未初始化"))?;
        let entry = manifest::catalog()
            .into_iter()
            .find(|m| m.id == profile.model)
            .ok_or_else(|| {
                TypexError::new(
                    ErrorCode::InvalidRequest,
                    format!("未知本地模型 {}", profile.model),
                )
            })?;
        let model_dir = dir.join("models").join(&entry.id);
        let threads = (std::thread::available_parallelism().map_or(4, |n| n.get() / 2)).max(1);
        match entry.engine {
            manifest::ModelEngine::Sherpa => Ok(Arc::new(stt_sense_voice::SenseVoiceStt::new(
                model_dir,
                threads as i32,
            ))),
            manifest::ModelEngine::Llama => {
                let gguf = entry
                    .files
                    .iter()
                    .find(|f| !f.name.starts_with("mmproj"))
                    .map(|f| model_dir.join(&f.name))
                    .ok_or_else(|| TypexError::new(ErrorCode::Internal, "清单缺主模型文件"))?;
                let mmproj = entry
                    .files
                    .iter()
                    .find(|f| f.name.starts_with("mmproj"))
                    .map(|f| model_dir.join(&f.name))
                    .ok_or_else(|| TypexError::new(ErrorCode::Internal, "清单缺 mmproj 文件"))?;
                Ok(Arc::new(stt_qwen_asr::QwenAsrStt::new(
                    gguf,
                    mmproj,
                    threads as i32,
                )))
            }
        }
    }

    pub fn build_llm(&self, profile: &ProviderProfile) -> Result<Arc<dyn LlmProvider>> {
        let key = self.resolve_secret(profile, "api_key")?;
        let client = self.http_client(profile.timeout_ms);
        match profile.kind {
            ProviderKind::ChatCompletions => Ok(Arc::new(
                ChatCompletionsLlm::new(
                    client,
                    profile.base_url.clone(),
                    key,
                    profile.model.clone(),
                )
                .with_headers(profile.extra_headers.clone()),
            )),
            ProviderKind::Responses => Ok(Arc::new(
                ResponsesLlm::new(client, profile.base_url.clone(), key, profile.model.clone())
                    .with_headers(profile.extra_headers.clone()),
            )),
            #[cfg(feature = "local-models")]
            ProviderKind::Local => {
                use crate::local::{llm_llama, manifest};
                let dir = self
                    .models_data_dir
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or_else(|| {
                        TypexError::new(ErrorCode::NotConfigured, "本地模型目录未初始化")
                    })?;
                let entry = manifest::catalog()
                    .into_iter()
                    .find(|m| m.id == profile.model)
                    .ok_or_else(|| {
                        TypexError::new(
                            ErrorCode::InvalidRequest,
                            format!("未知本地模型 {}", profile.model),
                        )
                    })?;
                // 槽位限制（ADR-22）：本地 LLM 只允许整理/翻译槽；问答槽仅当
                // profile 显式配置（设置中手动指向）时可用——兜底路径不会走到这。
                let gguf = entry
                    .files
                    .first()
                    .map(|f| dir.join("models").join(&entry.id).join(&f.name))
                    .ok_or_else(|| TypexError::new(ErrorCode::Internal, "清单缺模型文件"))?;
                Ok(Arc::new(llm_llama::LlamaLlm::new(
                    gguf,
                    llm_llama::LoadPolicy::Resident,
                )))
            }
            _ => Err(TypexError::new(
                ErrorCode::InvalidRequest,
                format!("{:?} 不是 LLM 类型", profile.kind),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::schema::SlotConfig;
    use crate::settings::secrets::MemoryStore;

    fn profile(id: &str, kind: ProviderKind) -> ProviderProfile {
        ProviderProfile {
            id: id.into(),
            slots: vec![SlotKind::Stt],
            kind,
            label: id.into(),
            base_url: "https://api.example.com/v1".into(),
            model: "m".into(),
            credentials: [(
                "api_key".to_string(),
                format!("keyring://typex/stt/{id}/api_key"),
            )]
            .into(),
            extra_headers: HashMap::new(),
            extra_form: HashMap::new(),
            timeout_ms: 30_000,
            options: HashMap::new(),
        }
    }

    fn setup() -> (ProviderRegistry, Arc<MemoryStore>) {
        let secrets = Arc::new(MemoryStore::default());
        secrets
            .set("keyring://typex/stt/p1/api_key", "sk-1")
            .unwrap();
        let mut s = Settings::default();
        s.profiles.push(profile("p1", ProviderKind::OpenaiCompat));
        s.slots.insert(
            SlotKind::Stt,
            SlotConfig {
                active_profile: Some("p1".into()),
            },
        );
        (ProviderRegistry::new(s, secrets.clone()), secrets)
    }

    #[test]
    fn stt_for_builds_and_caches() {
        let (reg, _) = setup();
        let a = reg.stt_for(SlotKind::Stt).unwrap();
        let b = reg.stt_for(SlotKind::Stt).unwrap();
        assert!(Arc::ptr_eq(&a, &b)); // 缓存命中
    }

    #[test]
    fn unconfigured_slot_yields_not_configured() {
        let (reg, _) = setup();
        let err = match reg.llm_for(SlotKind::Assistant) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert_eq!(err.code, ErrorCode::NotConfigured);
    }

    #[test]
    fn missing_secret_is_error_not_panic() {
        let secrets = Arc::new(MemoryStore::default()); // 没存密钥
        let mut s = Settings::default();
        s.profiles.push(profile("p1", ProviderKind::OpenaiCompat));
        s.slots.insert(
            SlotKind::Stt,
            SlotConfig {
                active_profile: Some("p1".into()),
            },
        );
        let reg = ProviderRegistry::new(s, secrets);
        assert!(reg.stt_for(SlotKind::Stt).is_err());
    }

    #[test]
    fn settings_change_invalidates_only_affected_profile() {
        let (reg, _) = setup();
        let a = reg.stt_for(SlotKind::Stt).unwrap();

        // 无关变更：缓存保留
        let mut s2 = reg.settings.lock().unwrap().clone();
        s2.general.autostart = false;
        reg.on_settings_changed(s2.clone());
        let b = reg.stt_for(SlotKind::Stt).unwrap();
        assert!(Arc::ptr_eq(&a, &b));

        // p1 base_url 变更：缓存失效重建
        s2.profiles[0].base_url = "https://other.example.com/v1".into();
        reg.on_settings_changed(s2);
        let c = reg.stt_for(SlotKind::Stt).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
    }

    /// ADR-20：问答槽无本地兜底——未配置一律 NotConfigured。
    #[test]
    fn assistant_slot_has_no_local_fallback() {
        let (reg, _) = setup(); // 仅配置了 Stt 槽
        let err = match reg.llm_for(SlotKind::Assistant) {
            Err(e) => e,
            Ok(_) => panic!("Assistant 槽不应有兜底"),
        };
        assert_eq!(err.code, ErrorCode::NotConfigured);
    }

    /// ADR-20：模型目录未注入 / 未下载时兜底同样报 NotConfigured（不 panic）。
    #[test]
    fn unconfigured_stt_without_local_models_is_not_configured() {
        let secrets = Arc::new(MemoryStore::default());
        let reg = ProviderRegistry::new(Settings::default(), secrets);
        let err = match reg.stt_for(SlotKind::Stt) {
            Err(e) => e,
            Ok(_) => panic!("未配置且无模型时不应成功"),
        };
        assert_eq!(err.code, ErrorCode::NotConfigured);
    }
}
