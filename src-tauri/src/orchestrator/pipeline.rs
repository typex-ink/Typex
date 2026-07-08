//! 听写/翻译/助手 三条流水线的「处理阶段」策略（06 §4）。
//!
//! 状态机只知道 `CallProcess`；本模块按 mode 选提示词与模型槽：
//! - Dictation：F-9 整理（整理层关闭/未配置/失败 → Degraded 直通原文，绝不阻塞）
//! - Translation：先按 F-9 开关预整理，再翻译（翻译失败 → Failed，HUD 提供注入原文）
//! - Assistant：在 assistant.rs 中先按 F-9 开关预整理语音指令

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

pub struct PreparedTranscript {
    pub text: String,
    pub degraded: bool,
}

/// 提示词渲染上下文：会话开始时采样，后续重试沿用同一份上下文。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptContext {
    pub target_app: Option<String>,
}

impl PromptContext {
    pub fn new(target_app: Option<String>) -> Self {
        Self {
            target_app: target_app.and_then(|app| {
                let app = app.trim();
                (!app.is_empty()).then(|| app.to_string())
            }),
        }
    }

    pub fn insert_values(&self, values: &mut HashMap<&'static str, String>) {
        if let Some(app) = &self.target_app {
            values.insert("{target_app}", app.clone());
        }
    }
}

pub async fn process(
    mode: SessionMode,
    transcript: String,
    settings: &Settings,
    registry: &Arc<ProviderRegistry>,
    prompt_context: &PromptContext,
) -> ProcessOutcome {
    match mode {
        SessionMode::Dictation => polish(transcript, settings, registry, prompt_context).await,
        SessionMode::Translation => translate(transcript, settings, registry, prompt_context).await,
        // F-3 的助手面板流式路径由 assistant.rs 接管；此处直通防御
        SessionMode::Assistant => ProcessOutcome::Done(transcript),
    }
}

/// 按听写的文本整理开关和提示词预处理 STT 文本。
///
/// 用于听写最终正文，也用于翻译文本和助手语音指令。关闭「文本整理」时不处理；
/// 开启但整理槽不可用/超时/报错/空输出时直通原文，不阻断下游功能。
pub async fn prepare_transcript(
    transcript: String,
    settings: &Settings,
    registry: &Arc<ProviderRegistry>,
    prompt_context: &PromptContext,
) -> PreparedTranscript {
    if !settings.dictation.polish_enabled {
        return PreparedTranscript {
            text: transcript,
            degraded: false,
        };
    }
    let llm = match registry.llm_for(SlotKind::Polish) {
        Ok(llm) => llm,
        Err(e) => {
            tracing::warn!("整理槽不可用，直通原文: {}", e.message);
            return PreparedTranscript {
                text: transcript,
                degraded: true,
            };
        }
    };

    let template = if settings.dictation.polish_prompt.is_empty() {
        prompt::POLISH_TEMPLATE
    } else {
        &settings.dictation.polish_prompt
    };
    let mut values = HashMap::new();
    values.insert("{transcript}", transcript.clone());
    prompt_context.insert_values(&mut values);
    if let Some(dictionary) = settings.dictionary.llm_context() {
        values.insert("{dictionary}", dictionary);
    }
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
        Ok(Ok(text)) if !text.trim().is_empty() => PreparedTranscript {
            text: text.trim().to_string(),
            degraded: false,
        },
        Ok(Ok(_)) => PreparedTranscript {
            text: transcript,
            degraded: true,
        },
        Ok(Err(e)) => {
            tracing::warn!("整理失败降级直通: {e}");
            PreparedTranscript {
                text: transcript,
                degraded: true,
            }
        }
        Err(_) => {
            tracing::warn!("整理超时降级直通");
            PreparedTranscript {
                text: transcript,
                degraded: true,
            }
        }
    }
}

/// F-9 文本整理：失败/超时/未配置一律降级直通（02 F-9 铁律）。
async fn polish(
    transcript: String,
    settings: &Settings,
    registry: &Arc<ProviderRegistry>,
    prompt_context: &PromptContext,
) -> ProcessOutcome {
    let prepared = prepare_transcript(transcript, settings, registry, prompt_context).await;
    if prepared.degraded {
        ProcessOutcome::Degraded(prepared.text)
    } else {
        ProcessOutcome::Done(prepared.text)
    }
}

/// F-2 翻译：双向判向在提示词内完成；失败 → Failed（HUD 注入原文降级）。
async fn translate(
    transcript: String,
    settings: &Settings,
    registry: &Arc<ProviderRegistry>,
    prompt_context: &PromptContext,
) -> ProcessOutcome {
    let llm = match registry.llm_for(SlotKind::Translate) {
        Ok(l) => l,
        Err(e) => return ProcessOutcome::Failed(e),
    };
    let transcript = prepare_transcript(transcript, settings, registry, prompt_context)
        .await
        .text;
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
    prompt_context.insert_values(&mut values);
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
