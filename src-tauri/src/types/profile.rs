//! Provider 配置档案类型（03 §6 配置 schema 的 Rust 形态）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
}

/// adapter 走向（03 §1）。`local` 是 v1.1 扩展位。
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
}

impl ProviderKind {
    pub fn is_stt(self) -> bool {
        matches!(self, ProviderKind::OpenaiCompat | ProviderKind::Volcengine)
    }
}

/// 一个配置档案。`credentials` 的值是 `keyring://` 引用，明文不落盘（03 §6）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ProviderProfile {
    pub id: String,
    /// 此档案适用的槽位（多槽共用连接时含多个）
    pub slots: Vec<SlotKind>,
    pub kind: ProviderKind,
    pub label: String,
    #[serde(default)]
    pub base_url: String,
    pub model: String,
    /// 字段名 → keyring 引用（map 结构为火山双凭据设计）
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
    30_000
}
