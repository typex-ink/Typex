//! 助手服务（F-3）：单次 LLM 调用（03 §4：无 Agent 层）。
//!
//! ADR-23 / ADR-29 分流：
//! - 语音指令先按听写文本整理开关走 F-9 预整理（关闭则直通）
//! - F-3a 选中文本 + 改写指令（`<rewrite>...</rewrite>`）→ 静默收全文 → `Rewrite`（orchestrator 注入替换选区，不弹窗）
//! - F-3a 选中文本 + 提问（`<answer>...</answer>` 或未命中 `<rewrite>` 的 Fail-safe）→ 呼出回答弹窗流式展示 → `HandedOff`
//! - F-3b 无选区提问 → 必为纯文本流式回答型，弹窗立即呼出流式展示 → `HandedOff`

use crate::error::{ErrorCode, Result, TypexError};
use crate::providers::ProviderError;
use crate::providers::ProviderRegistry;
use crate::providers::llm::{
    LlmDelta,
    output_protocol::{SelectionFinish, SelectionStreamParser},
    prompt,
};
use crate::settings::SettingsService;
use crate::types::SlotKind;
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// 回答弹窗事件回调：started（重置 + 指令回显）/ delta / done / error。
pub enum AssistantEvent {
    Started {
        request_id: u64,
        instruction: String,
        /// 选区字数（无选区 = None）
        selection_chars: Option<u32>,
        /// 读取选区失败降级为普通提问（05 §4 提示行）
        degraded: bool,
    },
    Delta {
        request_id: u64,
        text: String,
    },
    Done {
        request_id: u64,
        full_text: String,
    },
    Error {
        request_id: u64,
        error: TypexError,
    },
}

/// `run` 的结果，orchestrator 据此推进状态机。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantOutcome {
    /// 改写型：结果直接替换选区（→ ProcessResult → Inject）
    Rewrite(String),
    /// 回答型：完整回答为 Some；弹窗内错误为 None（→ AssistantHandedOff）。
    HandedOff(Option<String>),
}

pub struct AssistantService {
    pub settings: Arc<SettingsService>,
    pub registry: Arc<ProviderRegistry>,
    pub sink: Box<dyn Fn(AssistantEvent) + Send + Sync>,
    /// 呼出回答弹窗（app 层注入；参数 = 是否有选区，决定定位方式）
    pub show_panel: Box<dyn Fn(bool) -> BoxFuture<'static, ()> + Send + Sync>,
    /// 取消活动助手会话时隐藏回答窗。
    pub hide_panel: Box<dyn Fn() + Send + Sync>,
    next_id: AtomicU64,
}

impl AssistantService {
    pub fn new(
        settings: Arc<SettingsService>,
        registry: Arc<ProviderRegistry>,
        sink: Box<dyn Fn(AssistantEvent) + Send + Sync>,
        show_panel: Box<dyn Fn(bool) -> BoxFuture<'static, ()> + Send + Sync>,
        hide_panel: Box<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            settings,
            registry,
            sink,
            show_panel,
            hide_panel,
            next_id: AtomicU64::new(1),
        }
    }

    pub fn hide_panel(&self) {
        (self.hide_panel)();
    }

    /// 执行一次助手指令（语音转写稿）。弹窗呼出前的失败走 Err（HUD 失败态可重试）；
    /// 弹窗已呼出后的流中断在弹窗内展示并返回 `HandedOff`。
    pub async fn run(
        &self,
        instruction: String,
        selection: Option<String>,
        selection_read_failed: bool,
        prompt_context: super::pipeline::PromptContext,
    ) -> Result<AssistantOutcome> {
        let request_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let s = self.settings.get();
        let llm = self.registry.llm_for(SlotKind::Assistant)?;
        let instruction =
            super::pipeline::prepare_transcript(instruction, &s, &self.registry, &prompt_context)
                .await
                .text;

        // 有选区 = F-3a 处理协议（追加只读双标签输出协议）；无选区 = F-3b 纯文本问答协议。
        let (custom_system, built_in_system, output_protocol, content) =
            if let Some(selection) = selection.as_deref() {
                (
                    s.assistant.process_system_prompt.as_str(),
                    prompt::PROCESS_SYSTEM_PROMPT,
                    Some(prompt::PROCESS_OUTPUT_PROTOCOL),
                    prompt::selection_processing_request(
                        selection,
                        &instruction,
                        prompt_context.target_app.as_deref(),
                    ),
                )
            } else {
                (
                    s.assistant.ask_system_prompt.as_str(),
                    prompt::ASK_SYSTEM_PROMPT,
                    None,
                    prompt::question_request(&instruction, prompt_context.target_app.as_deref()),
                )
            };

        let req = prompt::single_turn_request_with_protocol(
            custom_system,
            built_in_system,
            output_protocol,
            content,
            0.3,
        );

        let ctx = RunCtx {
            request_id,
            instruction,
            selection_chars: selection.as_ref().map(|s| s.chars().count() as u32),
            had_selection: selection.is_some(),
            degraded: selection_read_failed && selection.is_none(),
            sink: self.sink.as_ref(),
            show_panel: self.show_panel.as_ref(),
        };
        drive(llm.complete(req), ctx).await
    }
}

/// `drive` 的依赖（便于对流式分流逻辑做纯单测）。
struct RunCtx<'a> {
    request_id: u64,
    instruction: String,
    selection_chars: Option<u32>,
    had_selection: bool,
    degraded: bool,
    sink: &'a (dyn Fn(AssistantEvent) + Send + Sync),
    show_panel: &'a (dyn Fn(bool) -> BoxFuture<'static, ()> + Send + Sync),
}

impl RunCtx<'_> {
    /// 呼出弹窗并回显指令（回答型确认的那一刻）。
    async fn open_panel(&self) {
        (self.show_panel)(self.had_selection).await;
        (self.sink)(AssistantEvent::Started {
            request_id: self.request_id,
            instruction: self.instruction.clone(),
            selection_chars: self.selection_chars,
            degraded: self.degraded,
        });
    }
}

/// 流式分流核心（ADR-23 / ADR-29）：
/// - 无选区：立即呼出弹窗，全程纯文本流式；
/// - 有选区：由 `SelectionStreamParser` 在流首部嗅探 `<rewrite>` / `<answer>`：
///   仅明确命中 `<rewrite>` 才静默收全文替换选区；其余（`<answer>` 或 Fail-safe）转弹窗流式。
async fn drive(
    mut stream: BoxStream<'static, std::result::Result<LlmDelta, ProviderError>>,
    ctx: RunCtx<'_>,
) -> Result<AssistantOutcome> {
    if !ctx.had_selection {
        return drive_unselected_question(stream, ctx).await;
    }

    let mut parser = SelectionStreamParser::new();
    let mut panel_open = false;

    while let Some(item) = stream.next().await {
        let delta = match item {
            Ok(d) => d,
            Err(e) => {
                if panel_open {
                    // 弹窗已呼出：错误就地展示，会话按已交接处理
                    (ctx.sink)(AssistantEvent::Error {
                        request_id: ctx.request_id,
                        error: e.into(),
                    });
                    return Ok(AssistantOutcome::HandedOff(None));
                }
                return Err(e.into());
            }
        };

        let step = parser.push(&delta.text);
        if let Some(route) = step.decided_route {
            tracing::debug!(route = ?route, "选区处理路由已判定");
        }
        if step.open_panel && !panel_open {
            ctx.open_panel().await;
            panel_open = true;
        }
        if let Some(text) = step.delta
            && panel_open
        {
            (ctx.sink)(AssistantEvent::Delta {
                request_id: ctx.request_id,
                text,
            });
        }
    }

    match parser.finish() {
        SelectionFinish::Rewrite(text) => Ok(AssistantOutcome::Rewrite(text)),
        SelectionFinish::Answer {
            open_panel,
            tail_delta,
            full_text,
        } => {
            if open_panel && !panel_open {
                ctx.open_panel().await;
                panel_open = true;
            }
            if let Some(text) = tail_delta
                && panel_open
            {
                (ctx.sink)(AssistantEvent::Delta {
                    request_id: ctx.request_id,
                    text,
                });
            }
            (ctx.sink)(AssistantEvent::Done {
                request_id: ctx.request_id,
                full_text: full_text.clone(),
            });
            Ok(AssistantOutcome::HandedOff(Some(full_text)))
        }
        SelectionFinish::Empty => {
            let err = TypexError::new(ErrorCode::ServerError, "回答为空");
            if panel_open {
                (ctx.sink)(AssistantEvent::Error {
                    request_id: ctx.request_id,
                    error: err,
                });
                return Ok(AssistantOutcome::HandedOff(None));
            }
            Err(err)
        }
    }
}

/// F-3b 无选区问答：立即呼出弹窗，全程纯文本流式展示。
async fn drive_unselected_question(
    mut stream: BoxStream<'static, std::result::Result<LlmDelta, ProviderError>>,
    ctx: RunCtx<'_>,
) -> Result<AssistantOutcome> {
    ctx.open_panel().await;
    let mut full = String::new();
    while let Some(item) = stream.next().await {
        let delta = match item {
            Ok(d) => d,
            Err(e) => {
                (ctx.sink)(AssistantEvent::Error {
                    request_id: ctx.request_id,
                    error: e.into(),
                });
                return Ok(AssistantOutcome::HandedOff(None));
            }
        };
        full.push_str(&delta.text);
        (ctx.sink)(AssistantEvent::Delta {
            request_id: ctx.request_id,
            text: delta.text,
        });
    }

    let display = full.trim().to_string();
    if display.is_empty() {
        let err = TypexError::new(ErrorCode::ServerError, "回答为空");
        (ctx.sink)(AssistantEvent::Error {
            request_id: ctx.request_id,
            error: err,
        });
        return Ok(AssistantOutcome::HandedOff(None));
    }

    (ctx.sink)(AssistantEvent::Done {
        request_id: ctx.request_id,
        full_text: display.clone(),
    });
    Ok(AssistantOutcome::HandedOff(Some(display)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;
    use std::sync::Mutex;

    /// 采集 sink 事件 + 弹窗呼出记录，跑 drive 并返回结果。
    struct Harness {
        events: Arc<Mutex<Vec<String>>>,
        panel_shows: Arc<Mutex<Vec<bool>>>,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(vec![])),
                panel_shows: Arc::new(Mutex::new(vec![])),
            }
        }

        fn run(
            &self,
            chunks: Vec<std::result::Result<&str, ProviderError>>,
            had_selection: bool,
        ) -> Result<AssistantOutcome> {
            // BoxStream<'static> 要求 owned 数据
            let owned: Vec<std::result::Result<LlmDelta, ProviderError>> = chunks
                .into_iter()
                .map(|c| {
                    c.map(|t| LlmDelta {
                        text: t.to_string(),
                    })
                })
                .collect();
            self.run_stream(futures_util::stream::iter(owned).boxed(), had_selection)
        }

        fn run_stream(
            &self,
            stream: BoxStream<'static, std::result::Result<LlmDelta, ProviderError>>,
            had_selection: bool,
        ) -> Result<AssistantOutcome> {
            let events = self.events.clone();
            let panel_shows = self.panel_shows.clone();
            let sink = move |e: AssistantEvent| {
                let s = match e {
                    AssistantEvent::Started { instruction, .. } => format!("started:{instruction}"),
                    AssistantEvent::Delta { text, .. } => format!("delta:{text}"),
                    AssistantEvent::Done { full_text, .. } => format!("done:{full_text}"),
                    AssistantEvent::Error { error, .. } => format!("error:{}", error.message),
                };
                events.lock().unwrap().push(s);
            };
            let show_panel = move |has_sel: bool| {
                panel_shows.lock().unwrap().push(has_sel);
                futures_util::future::ready(()).boxed()
            };
            let ctx = RunCtx {
                request_id: 1,
                instruction: "指令".into(),
                selection_chars: had_selection.then_some(3),
                had_selection,
                degraded: false,
                sink: &sink,
                show_panel: &show_panel,
            };
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap()
                .block_on(drive(stream, ctx))
        }

        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }

        fn panel_count(&self) -> usize {
            self.panel_shows.lock().unwrap().len()
        }
    }

    #[test]
    fn rewrite_tag_collects_silently_without_panel_and_strips_noise() {
        let h = Harness::new();
        let out = h
            .run(
                vec![
                    Ok("```xml\n<rew"),
                    Ok("rite>\n改写后的"),
                    Ok("正式文本。</rewrite>\n```\n尾部解释"),
                ],
                true,
            )
            .unwrap();
        assert_eq!(out, AssistantOutcome::Rewrite("改写后的正式文本。".into()));
        assert_eq!(h.panel_count(), 0, "改写型全程不弹窗");
        assert!(h.events().is_empty());
    }

    #[test]
    fn answer_tag_opens_panel_streams_and_strips_closing_tag_and_trailing_noise() {
        let h = Harness::new();
        let out = h
            .run(
                vec![
                    Ok("<answer> 原因"),
                    Ok("是超时。</ans"),
                    Ok("wer>\n希望对你有帮助"),
                ],
                true,
            )
            .unwrap();
        assert_eq!(
            out,
            AssistantOutcome::HandedOff(Some("原因是超时。".into()))
        );
        assert_eq!(h.panel_count(), 1);
        assert_eq!(
            h.events(),
            vec![
                "started:指令",
                "delta:原因",
                "delta:是超时。",
                "done:原因是超时。"
            ]
        );
    }

    #[test]
    fn answer_tag_split_across_chunks_with_leading_whitespace_is_detected() {
        let h = Harness::new();
        let out = h
            .run(
                vec![Ok("  <ANS"), Ok("WER>\n  "), Ok("回答体</answer>")],
                true,
            )
            .unwrap();
        assert_eq!(out, AssistantOutcome::HandedOff(Some("回答体".into())));
        assert_eq!(h.panel_count(), 1);
        assert_eq!(h.events().last().unwrap(), "done:回答体");
    }

    #[test]
    fn untagged_selection_output_fails_safe_to_answer_panel_instead_of_rewriting() {
        let h = Harness::new();
        let out = h
            .run(vec![Ok("这段文本可能"), Ok("表示超时错误。")], true)
            .unwrap();
        assert_eq!(
            out,
            AssistantOutcome::HandedOff(Some("这段文本可能表示超时错误。".into()))
        );
        assert_eq!(h.panel_count(), 1);
        assert_eq!(
            h.events(),
            vec![
                "started:指令",
                "delta:这段文本可能",
                "delta:表示超时错误。",
                "done:这段文本可能表示超时错误。"
            ]
        );
    }

    #[test]
    fn legacy_answer_prefix_fails_safe_to_panel_and_strips_prefix() {
        let h = Harness::new();
        let out = h
            .run(vec![Ok("ANSWER: 原因"), Ok("是超时。")], true)
            .unwrap();
        assert_eq!(
            out,
            AssistantOutcome::HandedOff(Some("原因是超时。".into()))
        );
        assert_eq!(h.panel_count(), 1);
        assert_eq!(h.events().last().unwrap(), "done:原因是超时。");
    }

    #[test]
    fn no_selection_is_always_answer_with_immediate_panel() {
        let h = Harness::new();
        let out = h.run(vec![Ok("任意"), Ok("输出")], false).unwrap();
        assert_eq!(out, AssistantOutcome::HandedOff(Some("任意输出".into())));
        assert_eq!(h.panel_count(), 1);
        assert_eq!(h.events()[0], "started:指令");
        assert_eq!(h.events().last().unwrap(), "done:任意输出");
    }

    #[test]
    fn short_partial_tag_output_fails_safe_as_answer() {
        // 宁可不替换也不误替换：未凑齐完整开标签的超短输出按回答展示
        let h = Harness::new();
        let out = h.run(vec![Ok("<re")], true).unwrap();
        assert_eq!(out, AssistantOutcome::HandedOff(Some("<re".into())));
        assert_eq!(h.panel_count(), 1);
        assert_eq!(h.events().last().unwrap(), "done:<re");
    }

    #[test]
    fn error_before_panel_is_err_for_hud() {
        let h = Harness::new();
        let r = h.run(vec![Err(ProviderError::Timeout)], true);
        assert!(r.is_err(), "弹窗未呼出的失败走 HUD 失败态");
        assert_eq!(h.panel_count(), 0);
    }

    #[test]
    fn error_after_panel_shown_in_panel() {
        let h = Harness::new();
        let out = h
            .run(vec![Ok("<answer>部分"), Err(ProviderError::Timeout)], true)
            .unwrap();
        assert_eq!(out, AssistantOutcome::HandedOff(None));
        assert!(h.events().last().unwrap().starts_with("error:"));
    }

    #[test]
    fn empty_or_empty_tag_output_with_selection_is_err() {
        for chunks in [
            vec![Ok("   ")],
            vec![Ok("<rewrite>   </rewrite>")],
            vec![Ok("<answer>   </answer>")],
            vec![Ok("ANSWER:   ")],
        ] {
            let h = Harness::new();
            let r = h.run(chunks, true);
            assert!(r.is_err());
            assert_eq!(h.panel_count(), 0);
        }
    }

    #[test]
    fn single_long_chunk_rewrite_does_not_fall_back_to_panel() {
        let h = Harness::new();
        let out = h
            .run(
                vec![Ok(
                    "<rewrite>这是一段超过二十四个字节的单块改写结果，必须静默替换选区且不弹出回答窗。</rewrite>",
                )],
                true,
            )
            .unwrap();
        assert_eq!(
            out,
            AssistantOutcome::Rewrite(
                "这是一段超过二十四个字节的单块改写结果，必须静默替换选区且不弹出回答窗。".into()
            )
        );
        assert_eq!(h.panel_count(), 0);
        assert!(h.events().is_empty());
    }
}
