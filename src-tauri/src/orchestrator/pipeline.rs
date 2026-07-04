//! 听写/翻译/助手 三条流水线的「处理阶段」策略（07 §4）。
//!
//! 状态机只知道 `CallProcess`；本模块决定按 mode 走哪条提示词/模型：
//! - Dictation：F-9 整理（失败降级直通原文 → ProcessDegraded）
//! - Translation：翻译提示词（失败 → ProcessFailed，HUD 提供注入原文）
//! - Assistant：CP-3 实现
//!
//! 整理层在 CP-1.5 接入 LlmProvider；当前为直通占位。

use crate::types::SessionMode;

/// 处理阶段的结果。
pub enum ProcessOutcome {
    /// 正常结果
    Done(String),
    /// 整理失败降级：直通原文（仅 Dictation）
    Degraded(String),
    /// 失败（Translation 等不可降级场景）
    Failed(crate::error::TypexError),
}

/// M0/M1 早期：无 LLM 配置时直通。CP-1.5 替换为真实整理调用。
pub async fn process(mode: SessionMode, transcript: String) -> ProcessOutcome {
    match mode {
        SessionMode::Dictation => ProcessOutcome::Done(transcript),
        SessionMode::Translation | SessionMode::Assistant => ProcessOutcome::Done(transcript),
    }
}
