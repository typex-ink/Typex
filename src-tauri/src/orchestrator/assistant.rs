//! 助手服务（F-3）：单次 LLM 调用（03 §4：无 Agent 层）。
//!
//! ADR-23 分流：
//! - 语音指令先按听写文本整理开关走 F-9 预整理（关闭则直通）
//! - F-3a 选中文本 + 改写指令 → 静默收全文 → `Rewrite`（orchestrator 注入替换选区，不弹窗）
//! - F-3a 选中文本 + 提问（`ANSWER:` 前缀，流首部嗅探）→ 呼出回答弹窗流式展示 → `HandedOff`
//! - F-3b 无选区提问 → 必为回答型，弹窗立即呼出流式展示 → `HandedOff`

use crate::error::{ErrorCode, Result, TypexError};
use crate::providers::ProviderError;
use crate::providers::ProviderRegistry;
use crate::providers::llm::{LlmDelta, LlmRequest, Msg, prompt};
use crate::settings::SettingsService;
use crate::types::SlotKind;
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const ASSISTANT_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(45);

/// 回答弹窗事件回调：started（重置 + 指令回显）/ delta / done / error。
pub enum AssistantEvent {
    Started {
        request_id: u64,
        instruction: String,
        /// 选区字数（无选区 = None）
        selection_chars: Option<u32>,
        /// 读取选区失败降级为普通提问（05 §4 / CP-6.13 提示行）
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
    /// 回答型：已交回答弹窗展示（含弹窗内错误），主会话结束（→ AssistantHandedOff）
    HandedOff,
}

pub struct AssistantService {
    pub settings: Arc<SettingsService>,
    pub registry: Arc<ProviderRegistry>,
    pub sink: Box<dyn Fn(AssistantEvent) + Send + Sync>,
    /// 呼出回答弹窗（app 层注入；参数 = 是否有选区，决定定位方式）
    pub show_panel: Box<dyn Fn(bool) -> BoxFuture<'static, ()> + Send + Sync>,
    next_id: AtomicU64,
}

impl AssistantService {
    pub fn new(
        settings: Arc<SettingsService>,
        registry: Arc<ProviderRegistry>,
        sink: Box<dyn Fn(AssistantEvent) + Send + Sync>,
        show_panel: Box<dyn Fn(bool) -> BoxFuture<'static, ()> + Send + Sync>,
    ) -> Self {
        Self {
            settings,
            registry,
            sink,
            show_panel,
            next_id: AtomicU64::new(1),
        }
    }

    /// 执行一次助手指令（语音转写稿）。弹窗呼出前的失败走 Err（HUD 失败态可重试）；
    /// 弹窗已呼出后的流中断在弹窗内展示并返回 `HandedOff`。
    pub async fn run(
        &self,
        instruction: String,
        selection: Option<String>,
        selection_read_failed: bool,
    ) -> Result<AssistantOutcome> {
        let request_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let s = self.settings.get();
        let llm = self.registry.llm_for(SlotKind::Assistant)?;
        let instruction = super::pipeline::prepare_transcript(instruction, &s, &self.registry)
            .await
            .text;

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
        values.insert("{instruction}", instruction.clone());
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

/// 流式分流核心（ADR-23）：
/// - 无选区：立即呼出弹窗，全程流式；
/// - 有选区：缓冲流首部直到能判定 `ANSWER:` 前缀——回答型转弹窗流式，改写型静默收全文。
async fn drive(
    stream: BoxStream<'static, std::result::Result<LlmDelta, ProviderError>>,
    ctx: RunCtx<'_>,
) -> Result<AssistantOutcome> {
    drive_with_idle_timeout(stream, ctx, ASSISTANT_STREAM_IDLE_TIMEOUT).await
}

async fn drive_with_idle_timeout(
    mut stream: BoxStream<'static, std::result::Result<LlmDelta, ProviderError>>,
    ctx: RunCtx<'_>,
    idle_timeout: Duration,
) -> Result<AssistantOutcome> {
    let prefix = prompt::ANSWER_PREFIX;
    let mut full = String::new();
    // 面板是否已呼出；有选区时延迟到前缀判定完成
    let mut panel_open = false;
    if !ctx.had_selection {
        ctx.open_panel().await;
        panel_open = true;
    }
    // 有选区时的判定结论：None = 仍在嗅探
    let mut is_rewrite: Option<bool> = if ctx.had_selection { None } else { Some(false) };

    loop {
        let item = match tokio::time::timeout(idle_timeout, stream.next()).await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(_) => {
                let err = TypexError::new(ErrorCode::Timeout, "助手回答超时");
                if panel_open {
                    (ctx.sink)(AssistantEvent::Error {
                        request_id: ctx.request_id,
                        error: err,
                    });
                    return Ok(AssistantOutcome::HandedOff);
                }
                return Err(err);
            }
        };
        let delta = match item {
            Ok(d) => d,
            Err(e) => {
                if panel_open {
                    // 弹窗已呼出：错误就地展示，会话按已交接处理
                    (ctx.sink)(AssistantEvent::Error {
                        request_id: ctx.request_id,
                        error: e.into(),
                    });
                    return Ok(AssistantOutcome::HandedOff);
                }
                return Err(e.into());
            }
        };
        full.push_str(&delta.text);

        if is_rewrite.is_none() {
            // 嗅探：首个非空白片段凑满前缀长度即可判定
            let lead = full.trim_start();
            if lead.len() >= prefix.len() {
                is_rewrite = Some(!lead.starts_with(prefix));
            } else if !prefix.starts_with(lead) {
                // 已有内容不再可能凑出前缀 → 提前判定为改写
                is_rewrite = Some(true);
            }
            if is_rewrite == Some(false) && !panel_open {
                // 回答型确认：呼出弹窗 + 释放缓冲（剥掉前缀）
                ctx.open_panel().await;
                panel_open = true;
                let buffered = full.trim_start();
                let buffered = buffered.strip_prefix(prefix).unwrap_or(buffered);
                if !buffered.is_empty() {
                    (ctx.sink)(AssistantEvent::Delta {
                        request_id: ctx.request_id,
                        text: buffered.trim_start().to_string(),
                    });
                }
            }
            continue;
        }

        if panel_open {
            (ctx.sink)(AssistantEvent::Delta {
                request_id: ctx.request_id,
                text: delta.text,
            });
        }
    }

    if full.trim().is_empty() {
        let err = TypexError::new(ErrorCode::ServerError, "回答为空");
        if panel_open {
            (ctx.sink)(AssistantEvent::Error {
                request_id: ctx.request_id,
                error: err,
            });
            return Ok(AssistantOutcome::HandedOff);
        }
        return Err(err);
    }

    match is_rewrite {
        Some(true) => Ok(AssistantOutcome::Rewrite(full.trim().to_string())),
        _ => {
            // 回答型（含流结束仍未凑满前缀长度的超短输出——按回答展示，宁可不替换也不误替换）
            let display = full.trim();
            let display = display.strip_prefix(prefix).unwrap_or(display).trim();
            if !panel_open {
                ctx.open_panel().await;
                (ctx.sink)(AssistantEvent::Delta {
                    request_id: ctx.request_id,
                    text: display.to_string(),
                });
            }
            (ctx.sink)(AssistantEvent::Done {
                request_id: ctx.request_id,
                full_text: display.to_string(),
            });
            Ok(AssistantOutcome::HandedOff)
        }
    }
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
            self.run_stream_with_timeout(
                futures_util::stream::iter(owned).boxed(),
                had_selection,
                ASSISTANT_STREAM_IDLE_TIMEOUT,
            )
        }

        fn run_stream_with_timeout(
            &self,
            stream: BoxStream<'static, std::result::Result<LlmDelta, ProviderError>>,
            had_selection: bool,
            idle_timeout: Duration,
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
                .block_on(drive_with_idle_timeout(stream, ctx, idle_timeout))
        }

        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }

        fn panel_count(&self) -> usize {
            self.panel_shows.lock().unwrap().len()
        }
    }

    #[test]
    fn rewrite_collects_silently_without_panel() {
        let h = Harness::new();
        let out = h.run(vec![Ok("改写后的"), Ok("正式文本。")], true).unwrap();
        assert_eq!(out, AssistantOutcome::Rewrite("改写后的正式文本。".into()));
        assert_eq!(h.panel_count(), 0, "改写型全程不弹窗");
        assert!(h.events().is_empty());
    }

    #[test]
    fn answer_prefix_opens_panel_and_streams() {
        let h = Harness::new();
        let out = h
            .run(vec![Ok("ANSWER: 原因"), Ok("是超时。")], true)
            .unwrap();
        assert_eq!(out, AssistantOutcome::HandedOff);
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
    fn answer_prefix_split_across_chunks_is_detected() {
        let h = Harness::new();
        let out = h
            .run(vec![Ok("ANS"), Ok("WER:"), Ok(" 回答体")], true)
            .unwrap();
        assert_eq!(out, AssistantOutcome::HandedOff);
        assert_eq!(h.events().last().unwrap(), "done:回答体");
    }

    #[test]
    fn leading_whitespace_before_prefix_still_answer() {
        let h = Harness::new();
        let out = h.run(vec![Ok("  ANSWER: 前导空格也算")], true).unwrap();
        assert_eq!(out, AssistantOutcome::HandedOff);
        assert_eq!(h.events().last().unwrap(), "done:前导空格也算");
    }

    #[test]
    fn no_selection_is_always_answer_with_immediate_panel() {
        let h = Harness::new();
        let out = h.run(vec![Ok("任意"), Ok("输出")], false).unwrap();
        assert_eq!(out, AssistantOutcome::HandedOff);
        assert_eq!(h.panel_count(), 1);
        assert_eq!(h.events()[0], "started:指令");
        assert_eq!(h.events().last().unwrap(), "done:任意输出");
    }

    #[test]
    fn short_output_shorter_than_prefix_shown_as_answer() {
        // 宁可不替换也不误替换：凑不满前缀长度的超短输出按回答展示
        let h = Harness::new();
        let out = h.run(vec![Ok("AN")], true).unwrap();
        assert_eq!(out, AssistantOutcome::HandedOff);
        assert_eq!(h.panel_count(), 1);
        assert_eq!(h.events().last().unwrap(), "done:AN");
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
            .run(vec![Ok("ANSWER: 部分"), Err(ProviderError::Timeout)], true)
            .unwrap();
        assert_eq!(out, AssistantOutcome::HandedOff);
        assert!(h.events().last().unwrap().starts_with("error:"));
    }

    #[test]
    fn idle_timeout_after_panel_shown_in_panel() {
        let h = Harness::new();
        let out = h
            .run_stream_with_timeout(
                futures_util::stream::pending().boxed(),
                false,
                Duration::from_millis(1),
            )
            .unwrap();
        assert_eq!(out, AssistantOutcome::HandedOff);
        assert_eq!(h.panel_count(), 1);
        assert_eq!(h.events(), vec!["started:指令", "error:助手回答超时"]);
    }

    #[test]
    fn idle_timeout_before_panel_is_err_for_hud() {
        let h = Harness::new();
        let r = h.run_stream_with_timeout(
            futures_util::stream::pending().boxed(),
            true,
            Duration::from_millis(1),
        );
        assert!(r.is_err(), "弹窗未呼出的超时走 HUD 失败态");
        assert_eq!(h.panel_count(), 0);
    }

    #[test]
    fn empty_output_with_selection_is_err() {
        let h = Harness::new();
        let r = h.run(vec![Ok("   ")], true);
        assert!(r.is_err());
        assert_eq!(h.panel_count(), 0);
    }
}
