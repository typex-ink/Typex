//! ProviderRegistry：由配置构造 provider 实例（03 §5 / 06 §5.1）。
//!
//! - 按 profile-id 惰性构建 + 缓存；settings 变更时失效重建
//! - 密钥从 profile.credentials 读取；日志与诊断导出必须脱敏

use crate::error::{ErrorCode, Result, TypexError};
use crate::providers::http;
use crate::providers::llm::{
    LlmProvider, TimedLlmProvider, chat_completions::ChatCompletionsLlm, responses::ResponsesLlm,
};
use crate::providers::stt::{
    SttProvider, TimedSttProvider, openai_compat::OpenAiCompatStt, volcengine::VolcengineStt,
};
use crate::settings::schema::Settings;
use crate::types::{ProviderCapability, ProviderKind, ProviderProfile, SlotKind};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct ProviderRegistry {
    /// profile-id → 已构建实例
    stt_cache: Mutex<HashMap<String, Arc<dyn SttProvider>>>,
    llm_cache: Mutex<HashMap<String, Arc<dyn LlmProvider>>>,
    /// 当前配置快照（settings watch 更新）
    settings: Mutex<Settings>,
    /// 本地模型存储根（app_data_dir；local-models）
    #[cfg_attr(not(feature = "local-models"), allow(dead_code))]
    models_data_dir: Mutex<Option<std::path::PathBuf>>,
}

const REASONING_EFFORT_VALUES: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh"];

fn profile_reasoning_effort(profile: &ProviderProfile) -> Option<String> {
    let effort = profile
        .options
        .get("reasoning_effort")
        .and_then(|v| v.as_str())?;
    REASONING_EFFORT_VALUES
        .contains(&effort)
        .then(|| effort.to_string())
}

fn reasoning_effort_enables_thinking(effort: &str) -> bool {
    effort != "none"
}

fn chat_completions_thinking_option(profile: &ProviderProfile) -> Option<bool> {
    if supports_enable_thinking_param(profile) {
        Some(profile_enable_thinking(profile))
    } else {
        None
    }
}

fn chat_completions_reasoning_effort(profile: &ProviderProfile) -> Option<String> {
    if supports_enable_thinking_param(profile) {
        None
    } else {
        profile_reasoning_effort(profile)
    }
}

fn profile_enable_thinking(profile: &ProviderProfile) -> bool {
    if let Some(effort) = profile_reasoning_effort(profile) {
        return reasoning_effort_enables_thinking(&effort);
    }
    profile
        .options
        .get("enable_thinking")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn supports_enable_thinking_param(profile: &ProviderProfile) -> bool {
    let base_url = profile.base_url.to_ascii_lowercase();
    if base_url.contains("siliconflow")
        || base_url.contains("dashscope")
        || base_url.contains("aliyuncs.com")
        || base_url.contains("bailian")
    {
        return true;
    }

    let model = profile.model.to_ascii_lowercase();
    (profile.options.contains_key("enable_thinking")
        || profile.options.contains_key("reasoning_effort"))
        && model.contains("qwen")
        && !base_url.contains("api.openai.com")
        && !base_url.contains("openrouter")
        && !base_url.contains("groq")
        && !base_url.contains("deepseek")
        && !base_url.contains("ollama")
}

impl ProviderRegistry {
    pub fn new(settings: Settings) -> Self {
        Self {
            stt_cache: Mutex::new(HashMap::new()),
            llm_cache: Mutex::new(HashMap::new()),
            settings: Mutex::new(settings),
            models_data_dir: Mutex::new(None),
        }
    }

    /// 设定本地模型存储根（runner 启动时注入 app_data_dir）。
    pub fn set_models_data_dir(&self, dir: std::path::PathBuf) {
        *self.models_data_dir.lock().unwrap() = Some(dir);
    }

    /// 配置变更：只清缓存中已消失/已变更的 profile（06 §5.1 惰性重建）。
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
            // 零配置兜底（ADR-20）：STT/整理/翻译功能未配置时指向本地服务配置
            // （模型已下载前提）；问答槽无兜底。
            return self.local_fallback_profile(slot);
        };
        let profile = s
            .profiles
            .iter()
            .find(|p| p.id == active)
            .cloned()
            .ok_or_else(|| {
                TypexError::new(ErrorCode::NotConfigured, format!("档案 {active} 不存在"))
            })?;
        if profile.capability != slot.capability() {
            return Err(TypexError::new(
                ErrorCode::InvalidRequest,
                format!("档案 {active} 不能用于 {slot:?} 槽"),
            ));
        }
        Ok(profile)
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
            capability: slot.capability(),
            kind: ProviderKind::Local,
            label: format!("本地 · {}", model.display_name),
            base_url: String::new(),
            model: model.id,
            credentials: HashMap::new(),
            extra_headers: HashMap::new(),
            extra_form: HashMap::new(),
            timeout_ms: crate::types::DEFAULT_PROVIDER_TIMEOUT_MS,
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
        let secret = profile
            .credentials
            .get(field)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                TypexError::new(
                    ErrorCode::NotConfigured,
                    format!("档案 {} 缺少凭据 {field}", profile.id),
                )
            })?;
        if secret.starts_with("keyring://") {
            return Err(TypexError::new(
                ErrorCode::NotConfigured,
                format!("档案 {} 使用旧版凭据引用，请重新保存 {field}", profile.id),
            ));
        }
        Ok(secret.to_string())
    }

    fn http_client(&self) -> reqwest::Client {
        let s = self.settings.lock().unwrap();
        http::build_client(s.general.proxy_mode, &s.general.proxy_url)
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
        if profile.capability != ProviderCapability::Stt {
            return Err(TypexError::new(
                ErrorCode::InvalidRequest,
                format!("{} 不是 STT 服务配置", profile.label),
            ));
        }
        if profile.timeout_ms == 0 {
            return Err(TypexError::new(
                ErrorCode::InvalidRequest,
                format!("{} 的调用超时必须大于 0", profile.label),
            ));
        }
        let provider: Arc<dyn SttProvider> = match profile.kind {
            ProviderKind::OpenaiCompat => {
                let key = self.resolve_secret(profile, "api_key")?;
                let client = self.http_client();
                Arc::new(
                    OpenAiCompatStt::new(
                        client,
                        profile.base_url.clone(),
                        key,
                        profile.model.clone(),
                    )
                    .with_extras(profile.extra_headers.clone(), profile.extra_form.clone()),
                )
            }
            ProviderKind::Volcengine => {
                let app_key = self.resolve_secret(profile, "app_key")?;
                let access_key = self.resolve_secret(profile, "access_token")?;
                let client = self.http_client();
                let mut p =
                    VolcengineStt::new(client, profile.base_url.clone(), app_key, access_key);
                if let Some(rid) = profile.options.get("resource_id").and_then(|v| v.as_str()) {
                    p = p.with_resource_id(rid);
                }
                Arc::new(p)
            }
            #[cfg(feature = "local-models")]
            ProviderKind::Local => self.build_local_stt(profile)?,
            _ => {
                return Err(TypexError::new(
                    ErrorCode::InvalidRequest,
                    format!("{:?} 不是 STT 类型", profile.kind),
                ));
            }
        };
        Ok(Arc::new(TimedSttProvider::new(
            provider,
            Duration::from_millis(profile.timeout_ms),
        )))
    }

    /// 本地 STT：按 model id 选引擎（sherpa / sherpa_whisper / llama mtmd）。
    #[cfg(feature = "local-models")]
    fn build_local_stt(&self, profile: &ProviderProfile) -> Result<Arc<dyn SttProvider>> {
        use crate::local::{manifest, stt_qwen_asr, stt_sense_voice, stt_whisper};
        let dir = self
            .models_data_dir
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| TypexError::new(ErrorCode::NotConfigured, "本地模型目录未初始化"))?;
        let entry = manifest::catalog_with_imported(&dir)
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
            manifest::ModelEngine::Sherpa => {
                let model = entry
                    .files
                    .iter()
                    .find(|f| f.name.ends_with(".onnx"))
                    .map(|f| model_dir.join(&f.name))
                    .ok_or_else(|| TypexError::new(ErrorCode::Internal, "清单缺 ONNX 模型文件"))?;
                let tokens = entry
                    .files
                    .iter()
                    .find(|f| f.name == "tokens.txt")
                    .map(|f| model_dir.join(&f.name))
                    .ok_or_else(|| TypexError::new(ErrorCode::Internal, "清单缺 tokens.txt"))?;
                Ok(Arc::new(stt_sense_voice::SenseVoiceStt::from_files(
                    model,
                    tokens,
                    threads as i32,
                )))
            }
            manifest::ModelEngine::SherpaWhisper => {
                let encoder = entry
                    .files
                    .iter()
                    .find(|f| f.name.contains("encoder") && f.name.ends_with(".onnx"))
                    .map(|f| model_dir.join(&f.name))
                    .ok_or_else(|| {
                        TypexError::new(ErrorCode::Internal, "清单缺 Whisper encoder 文件")
                    })?;
                let decoder = entry
                    .files
                    .iter()
                    .find(|f| f.name.contains("decoder") && f.name.ends_with(".onnx"))
                    .map(|f| model_dir.join(&f.name))
                    .ok_or_else(|| {
                        TypexError::new(ErrorCode::Internal, "清单缺 Whisper decoder 文件")
                    })?;
                let tokens = entry
                    .files
                    .iter()
                    .find(|f| f.name.ends_with("tokens.txt"))
                    .map(|f| model_dir.join(&f.name))
                    .ok_or_else(|| TypexError::new(ErrorCode::Internal, "清单缺 tokens.txt"))?;
                Ok(Arc::new(stt_whisper::WhisperStt::from_files(
                    encoder,
                    decoder,
                    tokens,
                    threads as i32,
                )))
            }
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
        if profile.capability != ProviderCapability::Llm {
            return Err(TypexError::new(
                ErrorCode::InvalidRequest,
                format!("{} 不是 LLM 服务配置", profile.label),
            ));
        }
        if profile.timeout_ms == 0 {
            return Err(TypexError::new(
                ErrorCode::InvalidRequest,
                format!("{} 的调用超时必须大于 0", profile.label),
            ));
        }
        let provider: Arc<dyn LlmProvider> = match profile.kind {
            ProviderKind::ChatCompletions => {
                let key = self.resolve_secret(profile, "api_key")?;
                let client = self.http_client();
                Arc::new(
                    ChatCompletionsLlm::new(
                        client,
                        profile.base_url.clone(),
                        key,
                        profile.model.clone(),
                    )
                    .with_headers(profile.extra_headers.clone())
                    .with_thinking(chat_completions_thinking_option(profile))
                    .with_reasoning_effort(chat_completions_reasoning_effort(profile)),
                )
            }
            ProviderKind::Responses => {
                let key = self.resolve_secret(profile, "api_key")?;
                let client = self.http_client();
                Arc::new(
                    ResponsesLlm::new(client, profile.base_url.clone(), key, profile.model.clone())
                        .with_headers(profile.extra_headers.clone())
                        .with_reasoning_effort(profile_reasoning_effort(profile)),
                )
            }
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
                let entry = manifest::catalog_with_imported(&dir)
                    .into_iter()
                    .find(|m| m.id == profile.model)
                    .ok_or_else(|| {
                        TypexError::new(
                            ErrorCode::InvalidRequest,
                            format!("未知本地模型 {}", profile.model),
                        )
                    })?;
                // ADR-20：问答槽不自动兜底；显式配置到 local 档案时照常可用。
                let gguf = entry
                    .files
                    .first()
                    .map(|f| dir.join("models").join(&entry.id).join(&f.name))
                    .ok_or_else(|| TypexError::new(ErrorCode::Internal, "清单缺模型文件"))?;
                // 加载策略（05 §5.1：常驻 / 用完即卸；编辑态下拉存 options.load_policy）
                let policy = match profile.options.get("load_policy").and_then(|v| v.as_str()) {
                    Some("unload_after_use") => llm_llama::LoadPolicy::UnloadAfterUse,
                    _ => llm_llama::LoadPolicy::Resident,
                };
                Arc::new(
                    llm_llama::LlamaLlm::new(gguf, policy)
                        .with_thinking(profile_enable_thinking(profile)),
                )
            }
            _ => {
                return Err(TypexError::new(
                    ErrorCode::InvalidRequest,
                    format!("{:?} 不是 LLM 类型", profile.kind),
                ));
            }
        };
        Ok(Arc::new(TimedLlmProvider::new(
            provider,
            Duration::from_millis(profile.timeout_ms),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::schema::SlotConfig;

    fn profile(id: &str, kind: ProviderKind) -> ProviderProfile {
        ProviderProfile {
            id: id.into(),
            capability: ProviderCapability::Stt,
            kind,
            label: id.into(),
            base_url: "https://api.example.com/v1".into(),
            model: "m".into(),
            credentials: [("api_key".to_string(), format!("sk-{id}"))].into(),
            extra_headers: HashMap::new(),
            extra_form: HashMap::new(),
            timeout_ms: 30_000,
            options: HashMap::new(),
        }
    }

    fn llm_profile(id: &str, base_url: &str) -> ProviderProfile {
        ProviderProfile {
            id: id.into(),
            capability: ProviderCapability::Llm,
            kind: ProviderKind::ChatCompletions,
            label: id.into(),
            base_url: base_url.into(),
            model: "Qwen/Qwen3-14B".into(),
            credentials: [("api_key".to_string(), format!("sk-{id}"))].into(),
            extra_headers: HashMap::new(),
            extra_form: HashMap::new(),
            timeout_ms: 30_000,
            options: HashMap::new(),
        }
    }

    fn setup() -> ProviderRegistry {
        let mut s = Settings::default();
        s.profiles.push(profile("p1", ProviderKind::OpenaiCompat));
        s.slots.insert(
            SlotKind::Stt,
            SlotConfig {
                active_profile: Some("p1".into()),
            },
        );
        ProviderRegistry::new(s)
    }

    #[test]
    fn stt_for_builds_and_caches() {
        let reg = setup();
        let a = reg.stt_for(SlotKind::Stt).unwrap();
        let b = reg.stt_for(SlotKind::Stt).unwrap();
        assert!(Arc::ptr_eq(&a, &b)); // 缓存命中
    }

    #[test]
    fn zero_stt_timeout_is_rejected_at_provider_boundary() {
        let reg = setup();
        let mut p = profile("invalid", ProviderKind::OpenaiCompat);
        p.timeout_ms = 0;

        let err = match reg.build_stt(&p) {
            Err(err) => err,
            Ok(_) => panic!("零超时不应构建 STT provider"),
        };
        assert_eq!(err.code, ErrorCode::InvalidRequest);
    }

    #[test]
    fn unconfigured_slot_yields_not_configured() {
        let reg = setup();
        let err = match reg.llm_for(SlotKind::Assistant) {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        assert_eq!(err.code, ErrorCode::NotConfigured);
    }

    #[test]
    fn missing_secret_is_error_not_panic() {
        let mut s = Settings::default();
        let mut p = profile("p1", ProviderKind::OpenaiCompat);
        p.credentials.clear();
        s.profiles.push(p);
        s.slots.insert(
            SlotKind::Stt,
            SlotConfig {
                active_profile: Some("p1".into()),
            },
        );
        let reg = ProviderRegistry::new(s);
        assert!(reg.stt_for(SlotKind::Stt).is_err());
    }

    #[test]
    fn legacy_keyring_secret_is_not_treated_as_api_key() {
        let mut s = Settings::default();
        let mut p = profile("p1", ProviderKind::OpenaiCompat);
        p.credentials
            .insert("api_key".into(), "keyring://typex/stt/p1/api_key".into());
        s.profiles.push(p);
        s.slots.insert(
            SlotKind::Stt,
            SlotConfig {
                active_profile: Some("p1".into()),
            },
        );
        let reg = ProviderRegistry::new(s);
        let err = match reg.stt_for(SlotKind::Stt) {
            Err(e) => e,
            Ok(_) => panic!("旧 keyring 引用不应被当作明文密钥"),
        };
        assert_eq!(err.code, ErrorCode::NotConfigured);
    }

    #[test]
    fn settings_change_invalidates_only_affected_profile() {
        let reg = setup();
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

    #[test]
    fn same_llm_profile_can_back_multiple_slots() {
        let mut s = Settings::default();
        s.profiles
            .push(llm_profile("shared", "https://api.example.com/v1"));
        for slot in [SlotKind::Translate, SlotKind::Assistant] {
            s.slots.insert(
                slot,
                SlotConfig {
                    active_profile: Some("shared".into()),
                },
            );
        }
        let reg = ProviderRegistry::new(s);

        let translate = reg.llm_for(SlotKind::Translate).unwrap();
        let assistant = reg.llm_for(SlotKind::Assistant).unwrap();
        assert!(Arc::ptr_eq(&translate, &assistant));
    }

    #[test]
    fn incompatible_profile_slot_is_rejected() {
        let reg = setup();
        let err = match reg.llm_for(SlotKind::Assistant) {
            Err(e) => e,
            Ok(_) => panic!("STT 档案不应可用于 LLM 槽"),
        };
        assert_eq!(err.code, ErrorCode::NotConfigured);

        let mut s = reg.settings.lock().unwrap().clone();
        s.slots.insert(
            SlotKind::Assistant,
            SlotConfig {
                active_profile: Some("p1".into()),
            },
        );
        reg.on_settings_changed(s);
        let err = match reg.llm_for(SlotKind::Assistant) {
            Err(e) => e,
            Ok(_) => panic!("STT 档案不应可用于 LLM 槽"),
        };
        assert_eq!(err.code, ErrorCode::InvalidRequest);
    }

    #[test]
    fn qwen_compatible_endpoint_sends_thinking_disabled_by_default() {
        let p = llm_profile("qwen", "https://api.siliconflow.cn/v1");
        assert_eq!(chat_completions_thinking_option(&p), Some(false));
    }

    #[test]
    fn qwen_compatible_endpoint_respects_thinking_option() {
        let mut p = llm_profile("qwen", "https://dashscope.aliyuncs.com/compatible-mode/v1");
        p.options
            .insert("enable_thinking".into(), serde_json::Value::Bool(true));
        assert_eq!(chat_completions_thinking_option(&p), Some(true));
    }

    #[test]
    fn qwen_compatible_endpoint_maps_reasoning_effort_to_thinking_bool() {
        let mut p = llm_profile("qwen", "https://dashscope.aliyuncs.com/compatible-mode/v1");
        p.options.insert(
            "reasoning_effort".into(),
            serde_json::Value::String("high".into()),
        );
        assert_eq!(chat_completions_thinking_option(&p), Some(true));
        assert_eq!(chat_completions_reasoning_effort(&p), None);

        p.options.insert(
            "reasoning_effort".into(),
            serde_json::Value::String("none".into()),
        );
        assert_eq!(chat_completions_thinking_option(&p), Some(false));
    }

    #[test]
    fn explicit_qwen_custom_endpoint_sends_thinking_param() {
        let mut p = llm_profile("qwen-custom", "https://qwen.example.com/v1");
        p.options
            .insert("enable_thinking".into(), serde_json::Value::Bool(false));
        assert_eq!(chat_completions_thinking_option(&p), Some(false));
    }

    #[test]
    fn profile_enable_thinking_defaults_false_and_reads_bool_or_effort() {
        let mut p = llm_profile("local-qwen", "");
        assert!(!profile_enable_thinking(&p));

        p.options
            .insert("enable_thinking".into(), serde_json::Value::Bool(true));
        assert!(profile_enable_thinking(&p));

        p.options.insert(
            "reasoning_effort".into(),
            serde_json::Value::String("none".into()),
        );
        assert!(!profile_enable_thinking(&p));

        p.options.insert(
            "reasoning_effort".into(),
            serde_json::Value::String("medium".into()),
        );
        assert!(profile_enable_thinking(&p));
    }

    #[test]
    fn non_qwen_endpoint_omits_thinking_param() {
        let mut p = llm_profile("openai", "https://api.openai.com/v1");
        p.options
            .insert("enable_thinking".into(), serde_json::Value::Bool(true));
        assert_eq!(chat_completions_thinking_option(&p), None);
    }

    #[test]
    fn non_qwen_endpoint_uses_generic_reasoning_effort() {
        let mut p = llm_profile("openai", "https://api.openai.com/v1");
        p.model = "gpt-5-mini".into();
        p.options.insert(
            "reasoning_effort".into(),
            serde_json::Value::String("high".into()),
        );
        assert_eq!(chat_completions_thinking_option(&p), None);
        assert_eq!(chat_completions_reasoning_effort(&p), Some("high".into()));
    }

    #[test]
    fn reasoning_effort_rejects_unknown_values() {
        let mut p = llm_profile("openai", "https://api.openai.com/v1");
        p.options.insert(
            "reasoning_effort".into(),
            serde_json::Value::String("maximum".into()),
        );
        assert_eq!(profile_reasoning_effort(&p), None);
    }

    /// ADR-20：问答槽无本地兜底——未配置一律 NotConfigured。
    #[test]
    fn assistant_slot_has_no_local_fallback() {
        let reg = setup(); // 仅配置了 Stt 槽
        let err = match reg.llm_for(SlotKind::Assistant) {
            Err(e) => e,
            Ok(_) => panic!("Assistant 槽不应有兜底"),
        };
        assert_eq!(err.code, ErrorCode::NotConfigured);
    }

    /// ADR-20：模型目录未注入 / 未下载时兜底同样报 NotConfigured（不 panic）。
    #[test]
    fn unconfigured_stt_without_local_models_is_not_configured() {
        let reg = ProviderRegistry::new(Settings::default());
        let err = match reg.stt_for(SlotKind::Stt) {
            Err(e) => e,
            Ok(_) => panic!("未配置且无模型时不应成功"),
        };
        assert_eq!(err.code, ErrorCode::NotConfigured);
    }

    #[cfg(feature = "local-models")]
    #[test]
    fn local_llm_build_does_not_require_api_key() {
        let reg = ProviderRegistry::new(Settings::default());
        let data_dir = tempfile::tempdir().unwrap();
        reg.set_models_data_dir(data_dir.path().to_path_buf());

        let profile = ProviderProfile {
            id: "local-llm".into(),
            capability: ProviderCapability::Llm,
            kind: ProviderKind::Local,
            label: "本地 · Qwen3.5".into(),
            base_url: String::new(),
            model: "qwen3.5-0.8b-q4".into(),
            credentials: HashMap::new(),
            extra_headers: HashMap::new(),
            extra_form: HashMap::new(),
            timeout_ms: 120_000,
            options: HashMap::new(),
        };

        reg.build_llm(&profile)
            .expect("本地 LLM 档案不应要求 api_key");
    }
}
