//! 听写/翻译/助手 三条流水线的「处理阶段」策略（07 §4）。
//!
//! 状态机只知道 `CallProcess`；本模块按 mode 选提示词与模型槽：
//! - Dictation：F-9 整理（整理层关闭/未配置/失败 → Degraded 直通原文，绝不阻塞）
//! - Translation：翻译提示词（失败 → Failed，HUD 提供注入原文）
//! - Assistant：CP-3 实现（当前直通）

use crate::error::{ErrorCode, TypexError};
use crate::providers::ProviderRegistry;
use crate::providers::llm::{LlmRequest, Msg, collect_text, prompt};
use crate::settings::schema::Settings;
use crate::types::{SessionMode, SlotKind};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// 处理阶段的结果。
pub enum ProcessOutcome {
    Done(String),
    /// 整理失败降级：直通原文（仅 Dictation）
    Degraded(String),
    Failed(TypexError),
}

/// 整理层延迟预算（02 F-9：≤ 500ms 推荐轻量模型；超时降级取 8s 硬上限）
const POLISH_TIMEOUT: Duration = Duration::from_secs(8);

pub async fn process(
    mode: SessionMode,
    transcript: String,
    settings: &Settings,
    registry: &Arc<ProviderRegistry>,
) -> ProcessOutcome {
    match mode {
        SessionMode::Dictation => polish(transcript, settings, registry).await,
        SessionMode::Translation => translate(transcript, settings, registry).await,
        // F-3 在 CP-3.x 接入助手面板流式路径；此处直通防御
        SessionMode::Assistant => ProcessOutcome::Done(transcript),
    }
}

/// F-9 文本整理：失败/超时/未配置一律降级直通（02 F-9 铁律）。
async fn polish(
    transcript: String,
    settings: &Settings,
    registry: &Arc<ProviderRegistry>,
) -> ProcessOutcome {
    if !settings.dictation.polish_enabled {
        return ProcessOutcome::Done(transcript); // 原样模式
    }
    let llm = match registry.llm_for(SlotKind::Polish) {
        Ok(l) => l,
        Err(_) => return ProcessOutcome::Degraded(transcript), // 未配置 → 直通
    };

    let template = if settings.dictation.polish_prompt.is_empty() {
        prompt::POLISH_TEMPLATE
    } else {
        &settings.dictation.polish_prompt
    };
    let mut values = HashMap::new();
    values.insert("{transcript}", transcript.clone());
    // {dictionary} 未启用（F-10 P2）→ 该行整体省略
    let rendered = prompt::render(template, &values);

    let req = LlmRequest {
        system: String::new(),
        messages: vec![Msg {
            role: "user".into(),
            content: rendered,
        }],
        temperature: 0.2,
        max_tokens: None,
    };
    match tokio::time::timeout(POLISH_TIMEOUT, collect_text(llm.as_ref(), req)).await {
        Ok(Ok(text)) if !text.trim().is_empty() => ProcessOutcome::Done(text.trim().to_string()),
        Ok(Ok(_)) => ProcessOutcome::Degraded(transcript), // 空结果 → 直通
        Ok(Err(e)) => {
            tracing::warn!("整理失败降级直通: {e}");
            ProcessOutcome::Degraded(transcript)
        }
        Err(_) => {
            tracing::warn!("整理超时降级直通");
            ProcessOutcome::Degraded(transcript)
        }
    }
}

/// F-2 翻译：双向判向在提示词内完成；失败 → Failed（HUD 注入原文降级）。
async fn translate(
    transcript: String,
    settings: &Settings,
    registry: &Arc<ProviderRegistry>,
) -> ProcessOutcome {
    let llm = match registry.llm_for(SlotKind::Translate) {
        Ok(l) => l,
        Err(e) => return ProcessOutcome::Failed(e),
    };
    let template = if settings.translation.translate_prompt.is_empty() {
        prompt::TRANSLATE_TEMPLATE
    } else {
        &settings.translation.translate_prompt
    };
    let mut values = HashMap::new();
    values.insert("{transcript}", transcript.clone());
    values.insert(
        "{source_language}",
        settings.translation.source_language.clone(),
    );
    values.insert(
        "{target_language}",
        settings.translation.target_language.clone(),
    );
    // 双向翻译子句：开关关闭时不注入值 → 模板中该行按可选段规则整体省略
    if settings.translation.bidirectional {
        values.insert(
            "{bidirectional_source}",
            settings.translation.source_language.clone(),
        );
        values.insert(
            "{bidirectional_target}",
            settings.translation.target_language.clone(),
        );
    }
    let rendered = prompt::render(template, &values);

    let req = LlmRequest {
        system: String::new(),
        messages: vec![Msg {
            role: "user".into(),
            content: rendered,
        }],
        temperature: 0.3,
        max_tokens: None,
    };
    match collect_text(llm.as_ref(), req).await {
        Ok(text) if !text.trim().is_empty() => ProcessOutcome::Done(text.trim().to_string()),
        Ok(_) => ProcessOutcome::Failed(TypexError::new(ErrorCode::ServerError, "翻译结果为空")),
        Err(e) => ProcessOutcome::Failed(e.into()),
    }
}
