//! ProviderRegistry：由配置构造 provider 实例（03 §5 / 07 §5.1）。
//!
//! - 按 profile-id 惰性构建 + 缓存；settings 变更时失效重建
//! - 密钥经 SecretStore 解析（keyring:// 引用 → 明文只在内存）

use crate::error::{ErrorCode, Result, TypexError};
use crate::providers::http;
use crate::providers::llm::{
    LlmProvider, chat_completions::ChatCompletionsLlm, responses::ResponsesLlm,
};
use crate::providers::stt::{SttProvider, openai_compat::OpenAiCompatStt};
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
}

impl ProviderRegistry {
    pub fn new(settings: Settings, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            secrets,
            stt_cache: Mutex::new(HashMap::new()),
            llm_cache: Mutex::new(HashMap::new()),
            settings: Mutex::new(settings),
        }
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
        let active = s
            .slots
            .get(&slot)
            .and_then(|c| c.active_profile.clone())
            .ok_or_else(|| {
                TypexError::new(ErrorCode::NotConfigured, format!("{slot:?} 槽未配置"))
            })?;
        s.profiles
            .iter()
            .find(|p| p.id == active)
            .cloned()
            .ok_or_else(|| {
                TypexError::new(ErrorCode::NotConfigured, format!("档案 {active} 不存在"))
            })
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
                // CP-2.x 之后按需实现 volcengine adapter
                Err(TypexError::new(
                    ErrorCode::InvalidRequest,
                    "volcengine adapter 尚未实现",
                ))
            }
            _ => Err(TypexError::new(
                ErrorCode::InvalidRequest,
                format!("{:?} 不是 STT 类型", profile.kind),
            )),
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
}
