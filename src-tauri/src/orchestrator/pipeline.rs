//! 听写/翻译/助手 三条流水线的「处理阶段」策略（06 §4）。
//!
//! 状态机只知道 `CallProcess`；本模块按 mode 选提示词与模型槽：
//! - Dictation：F-9 整理（整理层关闭/未配置/失败 → Degraded 直通原文，绝不阻塞）
//! - Translation：先按 F-9 开关预整理，再翻译（翻译失败 → Failed，HUD 提供注入原文）
//! - Assistant：在 assistant.rs 中先按 F-9 开关预整理语音指令

use crate::error::{ErrorCode, TypexError};
use crate::providers::ProviderRegistry;
use crate::providers::decision::{
    DecisionProvider, DecisionRequest, DecisionState, resolve_decision_instructions,
};
use crate::providers::llm::{LlmProvider, collect_text, output_protocol, prompt};
use crate::settings::schema::Settings;
use crate::types::{SessionMode, SlotKind};
use std::sync::Arc;

/// 处理阶段的结果。
pub enum ProcessOutcome {
    Done(String),
    /// 整理失败降级：直通原文（仅 Dictation）
    Degraded(String),
    Failed(TypexError),
}

pub struct PreparedTranscript {
    pub text: String,
    pub degraded: bool,
}

/// LLM 请求上下文：会话开始时采样，后续重试沿用同一份上下文。
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

/// 按听写的文本整理开关、可选整理决策模型和 system prompt 预处理 STT 文本。
///
/// 用于听写最终正文，也用于翻译文本和助手语音指令。关闭「文本整理」时不处理；
/// 若启用了「整理决策」槽且判定概率低于门限，则直接放行原始转写（`degraded = false`）；
/// 决策槽异常时回退至常规文本整理；整理槽不可用/超时/报错/空输出时直通原文，不阻断下游功能。
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
    // 先确认整理槽可用（纯内存缓存查询/轻量构造，零网络开销）：
    // 若整理槽本身未配置，绝不白白发起远端决策请求，且保持 degraded=true（「未整理」提示）。
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
    let decision = match registry.decision_for(SlotKind::Decision) {
        Ok(provider) => provider,
        Err(e) => {
            tracing::warn!(
                error_code = ?e.code,
                "整理决策槽配置不可用，回退至常规文本整理"
            );
            None
        }
    };
    if let Some(decision) = decision.as_deref()
        && !should_polish_transcript(&transcript, settings, decision, prompt_context).await
    {
        return PreparedTranscript {
            text: transcript,
            degraded: false,
        };
    }

    prepare_transcript_with_llm(transcript, settings, llm.as_ref(), prompt_context).await
}

fn normalized_threshold_f64(threshold: f32) -> f64 {
    (f64::from(threshold) * 1_000_000.0).round() / 1_000_000.0
}

async fn should_polish_transcript(
    transcript: &str,
    settings: &Settings,
    decision: &dyn DecisionProvider,
    prompt_context: &PromptContext,
) -> bool {
    let req = DecisionRequest {
        state: DecisionState {
            transcript: transcript.to_string(),
            target_app: prompt_context.target_app.clone(),
            dictionary_terms: settings.dictionary.normalized_terms(),
        },
        instructions: resolve_decision_instructions(&settings.dictation.polish_decision_prompt)
            .to_string(),
    };
    let threshold = normalized_threshold_f64(settings.dictation.polish_decision_threshold);
    match decision.decide(req).await {
        Ok(result) => {
            let needs_polish = result.probability >= threshold;
            tracing::debug!(
                stage = "polish_decision",
                needs_polish,
                probability = result.probability,
                threshold,
                transcript_len = transcript.len(),
                "整理决策完成"
            );
            needs_polish
        }
        Err(e) => {
            tracing::warn!(
                error_code = ?e.error_code(),
                http_status = ?e.http_status(),
                response_body_len = e.response_body_len(),
                "整理决策调用失败，回退至常规文本整理"
            );
            true
        }
    }
}

#[cfg(test)]
async fn prepare_transcript_with_providers(
    transcript: String,
    settings: &Settings,
    decision: Option<&dyn DecisionProvider>,
    llm: &dyn LlmProvider,
    prompt_context: &PromptContext,
) -> PreparedTranscript {
    if !settings.dictation.polish_enabled {
        return PreparedTranscript {
            text: transcript,
            degraded: false,
        };
    }
    if let Some(decision) = decision
        && !should_polish_transcript(&transcript, settings, decision, prompt_context).await
    {
        return PreparedTranscript {
            text: transcript,
            degraded: false,
        };
    }
    prepare_transcript_with_llm(transcript, settings, llm, prompt_context).await
}

async fn prepare_transcript_with_llm(
    transcript: String,
    settings: &Settings,
    llm: &dyn LlmProvider,
    prompt_context: &PromptContext,
) -> PreparedTranscript {
    let dictionary = settings.dictionary.llm_context();
    let content = prompt::dictation_cleanup_request(
        &transcript,
        prompt_context.target_app.as_deref(),
        dictionary.as_deref(),
    );

    let req = prompt::single_turn_request_with_protocol(
        &settings.dictation.polish_system_prompt,
        prompt::POLISH_SYSTEM_PROMPT,
        Some(prompt::POLISH_OUTPUT_PROTOCOL),
        content,
        0.2,
    );
    match collect_text(llm, req).await {
        Ok(raw) => {
            let extracted = output_protocol::extract_tagged_output(&raw, prompt::OUTPUT_TAG);
            if !extracted.tag_matched {
                tracing::debug!(
                    stage = "polish",
                    protocol_tag_present = false,
                    raw_len = raw.len(),
                    "整理输出未包含协议标签，回退原始文本"
                );
            }
            if !extracted.text.is_empty() {
                PreparedTranscript {
                    text: extracted.text,
                    degraded: false,
                }
            } else {
                PreparedTranscript {
                    text: transcript,
                    degraded: true,
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                error_code = ?e.error_code(),
                http_status = ?e.http_status(),
                response_body_len = e.response_body_len(),
                "整理失败降级直通"
            );
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

/// F-2 翻译：双向判向由 system prompt + XML 参数完成；失败 → Failed（HUD 注入原文降级）。
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
    translate_with_llm(transcript, settings, llm.as_ref(), prompt_context).await
}

async fn translate_with_llm(
    transcript: String,
    settings: &Settings,
    llm: &dyn LlmProvider,
    prompt_context: &PromptContext,
) -> ProcessOutcome {
    let content = prompt::translation_request(
        &transcript,
        &settings.translation.source_language,
        &settings.translation.target_language,
        settings.translation.bidirectional,
        prompt_context.target_app.as_deref(),
    );

    let req = prompt::single_turn_request_with_protocol(
        &settings.translation.translate_system_prompt,
        prompt::TRANSLATE_SYSTEM_PROMPT,
        Some(prompt::TRANSLATE_OUTPUT_PROTOCOL),
        content,
        0.3,
    );
    match collect_text(llm, req).await {
        Ok(raw) => {
            let extracted = output_protocol::extract_tagged_output(&raw, prompt::OUTPUT_TAG);
            if !extracted.tag_matched {
                tracing::debug!(
                    stage = "translate",
                    protocol_tag_present = false,
                    raw_len = raw.len(),
                    "翻译输出未包含协议标签，回退原始文本"
                );
            }
            if !extracted.text.is_empty() {
                ProcessOutcome::Done(extracted.text)
            } else {
                ProcessOutcome::Failed(TypexError::new(ErrorCode::ServerError, "翻译结果为空"))
            }
        }
        Err(e) => ProcessOutcome::Failed(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ProviderError;
    use crate::providers::llm::{LlmCapabilities, LlmDelta, LlmRequest};
    use futures_util::StreamExt;
    use futures_util::stream::{self, BoxStream};
    use std::sync::Mutex;

    struct StubLlm {
        response: String,
        last_request: Mutex<Option<LlmRequest>>,
    }

    impl StubLlm {
        fn new(response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
                last_request: Mutex::new(None),
            }
        }

        fn last_system(&self) -> String {
            self.last_request
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .system
                .clone()
        }
    }

    impl LlmProvider for StubLlm {
        fn complete(
            &self,
            req: LlmRequest,
        ) -> BoxStream<'static, std::result::Result<LlmDelta, ProviderError>> {
            *self.last_request.lock().unwrap() = Some(req);
            let text = self.response.clone();
            stream::iter([Ok(LlmDelta { text })]).boxed()
        }

        fn capabilities(&self) -> LlmCapabilities {
            LlmCapabilities { streaming: true }
        }
    }

    #[tokio::test]
    async fn polish_extracts_output_envelope_and_appends_protocol_to_custom_prompt() {
        let mut settings = Settings::default();
        settings.dictation.polish_system_prompt = "只整理文本。".into();
        let llm = StubLlm::new("好的，整理如下：\n<output>今天下午三点开会。</output>\n请查收");

        let prepared = prepare_transcript_with_llm(
            "嗯今天下午三点开会".into(),
            &settings,
            &llm,
            &PromptContext::default(),
        )
        .await;

        assert!(!prepared.degraded);
        assert_eq!(prepared.text, "今天下午三点开会。");
        let system = llm.last_system();
        assert!(system.starts_with("只整理文本。"));
        assert!(system.ends_with(prompt::POLISH_OUTPUT_PROTOCOL));
    }

    #[tokio::test]
    async fn polish_empty_output_tag_degrades_to_original_transcript() {
        let settings = Settings::default();
        let llm = StubLlm::new("<output>   </output>");

        let prepared =
            prepare_transcript_with_llm("嗯……".into(), &settings, &llm, &PromptContext::default())
                .await;

        assert!(prepared.degraded);
        assert_eq!(prepared.text, "嗯……");
    }

    #[tokio::test]
    async fn polish_without_protocol_tag_falls_back_to_raw_text() {
        let settings = Settings::default();
        let llm = StubLlm::new("  整理后的纯文本  ");

        let prepared = prepare_transcript_with_llm(
            "嗯整理后的纯文本".into(),
            &settings,
            &llm,
            &PromptContext::default(),
        )
        .await;

        assert!(!prepared.degraded);
        assert_eq!(prepared.text, "整理后的纯文本");
    }

    #[tokio::test]
    async fn translate_extracts_output_envelope_and_preserves_html() {
        let mut settings = Settings::default();
        settings.translation.translate_system_prompt = "忠实翻译。".into();
        let llm = StubLlm::new(
            "Here is the translation:\n<output><strong>Important</strong>: port must be 8080.</output>\nDone.",
        );

        let out = translate_with_llm(
            "<strong>重要</strong>：端口必须是 8080。".into(),
            &settings,
            &llm,
            &PromptContext::default(),
        )
        .await;

        match out {
            ProcessOutcome::Done(text) => {
                assert_eq!(text, "<strong>Important</strong>: port must be 8080.");
            }
            _ => panic!("expected ProcessOutcome::Done"),
        }
        let system = llm.last_system();
        assert!(system.starts_with("忠实翻译。"));
        assert!(system.ends_with(prompt::TRANSLATE_OUTPUT_PROTOCOL));
    }

    #[tokio::test]
    async fn translate_empty_output_tag_returns_failed() {
        let settings = Settings::default();
        let llm = StubLlm::new("<output></output>");

        let out =
            translate_with_llm("你好".into(), &settings, &llm, &PromptContext::default()).await;

        assert!(matches!(out, ProcessOutcome::Failed(err) if err.code == ErrorCode::ServerError));
    }

    struct StubDecision {
        outcome: std::result::Result<f64, ProviderError>,
        calls: std::sync::atomic::AtomicUsize,
        last_request: Mutex<Option<DecisionRequest>>,
    }

    impl StubDecision {
        fn ok(probability: f64) -> Self {
            Self {
                outcome: Ok(probability),
                calls: std::sync::atomic::AtomicUsize::new(0),
                last_request: Mutex::new(None),
            }
        }

        fn err(error: ProviderError) -> Self {
            Self {
                outcome: Err(error),
                calls: std::sync::atomic::AtomicUsize::new(0),
                last_request: Mutex::new(None),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl DecisionProvider for StubDecision {
        async fn decide(
            &self,
            req: DecisionRequest,
        ) -> std::result::Result<crate::providers::decision::DecisionResult, ProviderError>
        {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.last_request.lock().unwrap() = Some(req);
            self.outcome
                .clone()
                .map(|probability| crate::providers::decision::DecisionResult { probability })
        }
    }

    #[tokio::test]
    async fn decision_below_threshold_skips_polish_llm_without_degraded_flag() {
        let mut settings = Settings::default();
        settings.dictation.polish_decision_threshold = 0.50;
        settings.dictionary.terms = vec!["Typex".into()];
        let decision = StubDecision::ok(0.12);
        let llm = StubLlm::new("<output>不应被调用</output>");

        let prepared = prepare_transcript_with_providers(
            "好的".into(),
            &settings,
            Some(&decision),
            &llm,
            &PromptContext::new(Some("Slack".into())),
        )
        .await;

        assert!(!prepared.degraded);
        assert_eq!(prepared.text, "好的");
        assert_eq!(decision.call_count(), 1);
        assert!(llm.last_request.lock().unwrap().is_none());
        let captured = decision.last_request.lock().unwrap().clone().unwrap();
        assert_eq!(captured.state.transcript, "好的");
        assert_eq!(captured.state.target_app.as_deref(), Some("Slack"));
        assert_eq!(captured.state.dictionary_terms, vec!["Typex"]);
    }

    #[tokio::test]
    async fn decision_at_or_above_threshold_invokes_polish_llm() {
        let mut settings = Settings::default();
        settings.dictation.polish_decision_threshold = 0.30;
        settings.dictation.polish_decision_prompt = "自定义判定指令".into();
        let decision = StubDecision::ok(0.30);
        let llm = StubLlm::new("<output>今天下午四点开会。</output>");

        let prepared = prepare_transcript_with_providers(
            "嗯今天下午三点不对四点开会".into(),
            &settings,
            Some(&decision),
            &llm,
            &PromptContext::default(),
        )
        .await;

        assert!(!prepared.degraded);
        assert_eq!(prepared.text, "今天下午四点开会。");
        assert_eq!(decision.call_count(), 1);
        assert!(llm.last_request.lock().unwrap().is_some());
        assert_eq!(
            decision
                .last_request
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .instructions,
            "自定义判定指令"
        );
    }

    #[tokio::test]
    async fn decision_error_falls_back_to_polish_llm_and_verbatim_skips_both() {
        let mut settings = Settings::default();
        let failing_decision = StubDecision::err(ProviderError::Timeout);
        let llm = StubLlm::new("<output>整理完成。</output>");

        let prepared = prepare_transcript_with_providers(
            "嗯整理完成".into(),
            &settings,
            Some(&failing_decision),
            &llm,
            &PromptContext::default(),
        )
        .await;
        assert!(!prepared.degraded);
        assert_eq!(prepared.text, "整理完成。");
        assert_eq!(failing_decision.call_count(), 1);
        assert!(llm.last_request.lock().unwrap().is_some());

        settings.dictation.polish_enabled = false;
        let skipped_decision = StubDecision::ok(0.99);
        let untouched_llm = StubLlm::new("<output>不会调用</output>");
        let verbatim = prepare_transcript_with_providers(
            "原样直出".into(),
            &settings,
            Some(&skipped_decision),
            &untouched_llm,
            &PromptContext::default(),
        )
        .await;
        assert!(!verbatim.degraded);
        assert_eq!(verbatim.text, "原样直出");
        assert_eq!(skipped_decision.call_count(), 0);
        assert!(untouched_llm.last_request.lock().unwrap().is_none());
    }
}
