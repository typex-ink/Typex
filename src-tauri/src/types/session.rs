//! 会话快照类型：Rust 状态机 → 前端 HUD 的唯一渲染依据（07 §5.2）。

use crate::error::ErrorCode;
use serde::{Deserialize, Serialize};

/// 会话模式：三大功能共享同一状态机（07 §5.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Dictation,
    Translation,
    Assistant,
}

/// 会话阶段（对前端可见的投影；内部状态机携带 payload，见 orchestrator/session.rs）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Idle,
    Recording,
    Transcribing,
    Processing,
    Injecting,
    Success,
    Failed,
}

/// 失败发生在哪一步（决定 HUD 可提供的兜底动作，05 §3.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum FailedStage {
    Recording,
    Transcribing,
    Processing,
    Injecting,
}

/// 每次 phase 变更推送给前端的完整快照（event `session://snapshot`）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SessionSnapshot {
    pub session_id: u64,
    pub mode: SessionMode,
    pub phase: SessionPhase,
    /// 录音已进行毫秒数（Recording 态；HUD 计时）
    pub recording_ms: u64,
    /// 整理层是否关闭（HUD「原样」小字，05 §3.2）
    pub verbatim: bool,
    /// 翻译方向徽标文本用（如 "中 → EN"）
    pub translation_direction: Option<String>,
    /// Failed 态：错误码 + 发生阶段
    pub error: Option<ErrorCode>,
    pub failed_stage: Option<FailedStage>,
    /// STT 已成功时为 true——HUD 显示「复制原文」兜底（不丢话铁律）
    pub has_transcript: bool,
    /// 整理失败降级注入原文时为 true——HUD 小字「未整理」
    pub unpolished: bool,
    /// 处理中文案键（"transcribing" / "polishing" / "translating" / "thinking"）
    pub processing_step: Option<String>,
    /// 重按忽略提示（05 §3.3）：HUD 轻晃 + 「正在处理上一条…」微文案
    pub busy_hint: bool,
}

impl SessionSnapshot {
    pub fn idle() -> Self {
        Self {
            session_id: 0,
            mode: SessionMode::Dictation,
            phase: SessionPhase::Idle,
            recording_ms: 0,
            verbatim: false,
            translation_direction: None,
            error: None,
            failed_stage: None,
            has_transcript: false,
            unpolished: false,
            processing_step: None,
            busy_hint: false,
        }
    }
}
