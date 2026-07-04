//! 全部 IPC event 定义与 emit 封装（07 §10.2）。
//! 命名规范：`域://kebab-case`；载荷全部为 types/ 中的 struct。

use crate::types::SessionSnapshot;
use serde::{Deserialize, Serialize};
use tauri_specta::Event;

/// `session://snapshot` — HUD/托盘渲染依据。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct SessionSnapshotEvent(pub SessionSnapshot);

/// `session://audio-level` — HUD 波形（50ms 节流）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct AudioLevelEvent(pub Vec<f32>);

/// `settings://changed` — 全窗口设置同步。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct SettingsChangedEvent(pub crate::settings::schema::Settings);

/// `assistant://delta` — 助手面板流式渲染。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct AssistantDeltaEvent {
    pub request_id: u32,
    pub text_delta: String,
}

/// `assistant://done` — 面板动作行（kind 决定是否可替换选区）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct AssistantDoneEvent {
    pub request_id: u32,
    pub kind: crate::orchestrator::assistant::AnswerKind,
    pub full_text: String,
    /// 呼出时读到的选中文本长度（上下文芯片显示；0 = 无选区）
    pub selection_chars: u32,
}

/// `assistant://error`
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct AssistantErrorEvent {
    pub request_id: u32,
    pub error: crate::error::TypexError,
}

/// `assistant://context` — 呼出面板时的上下文信息（选中文本芯片）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct AssistantContextEvent {
    pub selection_chars: u32,
    pub selection_preview: String,
}
