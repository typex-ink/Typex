//! 助手服务（F-3）：单次 LLM 调用 + 流式推送到面板（03 §4：无 Agent 层）。
//!
//! F-3a：选中文本 + 指令 → 处理提示词 → 改写(替换选区) / ANSWER:(仅展示)
//! F-3b：无选区提问 → 问答提示词 → 面板流式展示

use crate::error::{ErrorCode, Result, TypexError};
use crate::providers::ProviderRegistry;
use crate::providers::llm::{LlmRequest, Msg, prompt};
use crate::settings::SettingsService;
use crate::types::SlotKind;
use futures_util::StreamExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// 面板事件回调：delta / done(kind) / error。
pub enum AssistantEvent {
    Delta {
        request_id: u64,
        text: String,
    },
    Done {
        request_id: u64,
        kind: AnswerKind,
        full_text: String,
    },
    Error {
        request_id: u64,
        error: TypexError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AnswerKind {
    /// 改写型：结果可直接替换选区
    Rewrite,
    /// 回答型（ANSWER: 前缀 / 无选区提问）：仅展示
    Answer,
}

pub struct AssistantService {
    pub settings: Arc<SettingsService>,
    pub registry: Arc<ProviderRegistry>,
    pub sink: Box<dyn Fn(AssistantEvent) + Send + Sync>,
    next_id: AtomicU64,
}

impl AssistantService {
    pub fn new(
        settings: Arc<SettingsService>,
        registry: Arc<ProviderRegistry>,
        sink: Box<dyn Fn(AssistantEvent) + Send + Sync>,
    ) -> Self {
        Self {
            settings,
            registry,
            sink,
            next_id: AtomicU64::new(1),
        }
    }

    /// 发起单轮提问（07 §10.1 ask_assistant）。返回 request_id。
    /// selection: 呼出时读到的选中文本（F-3a 上下文）。
    pub fn ask(self: &Arc<Self>, instruction: String, selection: Option<String>) -> Result<u64> {
        let request_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let s = self.settings.get();
        let llm = self.registry.llm_for(SlotKind::Assistant)?;

        // 选提示词：有选区 = 处理模板（F-3a）；无选区 = 问答模板（F-3b）
        let (template, custom) = if selection.is_some() {
            (prompt::PROCESS_TEMPLATE, s.assistant.process_prompt.clone())
        } else {
            (prompt::ASK_TEMPLATE, s.assistant.ask_prompt.clone())
        };
        let template = if custom.is_empty() {
            template.to_string()
        } else {
            custom
        };

        let mut values = std::collections::HashMap::new();
        values.insert("{instruction}", instruction);
        if let Some(sel) = &selection {
            values.insert("{selection}", sel.clone());
        }
        let rendered = prompt::render(&template, &values);

        let req = LlmRequest {
            system: String::new(),
            messages: vec![Msg {
                role: "user".into(),
                content: rendered,
            }],
            temperature: 0.3,
            max_tokens: None,
        };

        let this = self.clone();
        let had_selection = selection.is_some();
        tokio::spawn(async move {
            let mut stream = llm.complete(req);
            let mut full = String::new();
            while let Some(item) = stream.next().await {
                match item {
                    Ok(delta) => {
                        full.push_str(&delta.text);
                        (this.sink)(AssistantEvent::Delta {
                            request_id,
                            text: delta.text,
                        });
                    }
                    Err(e) => {
                        (this.sink)(AssistantEvent::Error {
                            request_id,
                            error: e.into(),
                        });
                        return;
                    }
                }
            }
            if full.trim().is_empty() {
                (this.sink)(AssistantEvent::Error {
                    request_id,
                    error: TypexError::new(ErrorCode::ServerError, "回答为空"),
                });
                return;
            }
            let kind = classify(&full, had_selection);
            let display = full
                .trim()
                .strip_prefix(prompt::ANSWER_PREFIX)
                .unwrap_or(full.trim());
            (this.sink)(AssistantEvent::Done {
                request_id,
                kind,
                full_text: display.trim().to_string(),
            });
        });
        Ok(request_id)
    }
}

/// 「改写 vs 回答」判定（02 F-3a：宁可不替换也不误替换）。
fn classify(output: &str, had_selection: bool) -> AnswerKind {
    if !had_selection {
        return AnswerKind::Answer;
    }
    if output.trim_start().starts_with(prompt::ANSWER_PREFIX) {
        return AnswerKind::Answer;
    }
    AnswerKind::Rewrite
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_selection_is_always_answer() {
        assert_eq!(classify("任意输出", false), AnswerKind::Answer);
        assert_eq!(classify("ANSWER: 回答", false), AnswerKind::Answer);
    }

    #[test]
    fn answer_prefix_prevents_replace() {
        assert_eq!(
            classify("ANSWER: 这段报错的原因是……", true),
            AnswerKind::Answer
        );
        assert_eq!(classify("  ANSWER: 前导空格也算", true), AnswerKind::Answer);
    }

    #[test]
    fn rewrite_without_prefix() {
        assert_eq!(classify("改写后的正式文本。", true), AnswerKind::Rewrite);
    }
}
