//! Provider 配置档案类型（03 §6 配置 schema 的 Rust 形态）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_PROVIDER_TIMEOUT_MS: u64 = 60_000;

/// 四个模型槽位（02 F-4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Stt,
    Polish,
    Translate,
    Assistant,
}

impl SlotKind {
    pub const ALL: [SlotKind; 4] = [
        SlotKind::Stt,
        SlotKind::Polish,
        SlotKind::Translate,
        SlotKind::Assistant,
    ];

    pub fn capability(self) -> ProviderCapability {
        match self {
            SlotKind::Stt => ProviderCapability::Stt,
            SlotKind::Polish | SlotKind::Translate | SlotKind::Assistant => ProviderCapability::Llm,
        }
    }
}

/// 服务配置能力：语音转文字或文本模型（02 F-4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapability {
    Stt,
    Llm,
}

/// adapter 走向（03 §1）。`local` 表示本地推理 adapter。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// STT：multipart /audio/transcriptions
    OpenaiCompat,
    /// STT：火山/豆包极速版 flash（双凭据）
    Volcengine,
    /// LLM：OpenAI Chat Completions
    ChatCompletions,
    /// LLM：OpenAI Responses
    Responses,
    /// 本地推理（ADR-20；无 base_url/凭据，model = 模型库 id）
    Local,
}

impl ProviderKind {
    pub fn is_stt(self) -> bool {
        matches!(
            self,
            ProviderKind::OpenaiCompat | ProviderKind::Volcengine | ProviderKind::Local
        )
    }
}

/// 本地模型下载源（03 §8）。`Auto` 保持双源自动兜底。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
pub enum ModelDownloadSource {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "huggingface")]
    HuggingFace,
    #[serde(rename = "modelscope")]
    ModelScope,
}

/// 一个配置档案。`credentials` 保存各 provider 需要的敏感配置值（03 §6）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ProviderProfile {
    pub id: String,
    /// 此服务配置提供的能力；功能槽位只保存指向 profile id 的指针。
    pub capability: ProviderCapability,
    pub kind: ProviderKind,
    pub label: String,
    #[serde(default)]
    pub base_url: String,
    pub model: String,
    /// 字段名 → 凭据值（map 结构为火山双凭据设计）
    #[serde(default)]
    pub credentials: HashMap<String, String>,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    #[serde(default)]
    pub extra_form: HashMap<String, String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    /// 槽位/adapter 相关自由选项（language、temperature、resource_id…）
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,
}

fn default_timeout_ms() -> u64 {
    DEFAULT_PROVIDER_TIMEOUT_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_json(timeout_ms: Option<u64>) -> serde_json::Value {
        let mut profile = serde_json::json!({
            "id": "llm",
            "capability": "llm",
            "kind": "chat_completions",
            "label": "LLM",
            "model": "model"
        });
        if let Some(timeout_ms) = timeout_ms {
            profile["timeout_ms"] = timeout_ms.into();
        }
        profile
    }

    #[test]
    fn missing_timeout_uses_current_default() {
        let profile: ProviderProfile = serde_json::from_value(profile_json(None)).unwrap();
        assert_eq!(profile.timeout_ms, DEFAULT_PROVIDER_TIMEOUT_MS);
    }

    #[test]
    fn explicit_timeout_is_preserved() {
        let profile: ProviderProfile = serde_json::from_value(profile_json(Some(30_000))).unwrap();
        assert_eq!(profile.timeout_ms, 30_000);
    }
}
