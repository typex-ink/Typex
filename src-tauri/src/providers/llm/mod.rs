//! LlmProvider trait + PromptKit（03 §3）。
pub mod chat_completions;
pub mod prompt;
pub mod responses;

use super::ProviderError;
use futures_util::stream::BoxStream;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Msg {
    pub role: String, // "user" | "assistant"
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub system: String,
    pub messages: Vec<Msg>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

/// 流式增量。
#[derive(Debug, Clone)]
pub struct LlmDelta {
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct LlmCapabilities {
    pub streaming: bool,
}

pub trait LlmProvider: Send + Sync {
    /// 单轮任务型调用；流式返回 delta（03 §3）。
    fn complete(&self, req: LlmRequest) -> BoxStream<'static, Result<LlmDelta, ProviderError>>;
    fn capabilities(&self) -> LlmCapabilities;
}

/// 为一个模型服务的所有调用统一施加 profile 总时限。
///
/// deadline 覆盖从调用流首次轮询到完整流结束；收到 delta 不会重置计时。
pub struct TimedLlmProvider {
    inner: Arc<dyn LlmProvider>,
    timeout: Duration,
}

impl TimedLlmProvider {
    pub fn new(inner: Arc<dyn LlmProvider>, timeout: Duration) -> Self {
        Self { inner, timeout }
    }
}

impl LlmProvider for TimedLlmProvider {
    fn complete(&self, req: LlmRequest) -> BoxStream<'static, Result<LlmDelta, ProviderError>> {
        use futures_util::StreamExt;

        let mut stream = self.inner.complete(req);
        let timeout = self.timeout;
        async_stream::try_stream! {
            let deadline = tokio::time::Instant::now()
                .checked_add(timeout)
                .ok_or_else(|| ProviderError::InvalidRequest("模型服务调用超时过大".into()))?;
            loop {
                let next = match tokio::time::timeout_at(deadline, stream.next()).await {
                    Ok(next) => next,
                    Err(_) => Err(ProviderError::Timeout)?,
                };
                match next {
                    Some(delta) => yield delta?,
                    None => break,
                }
            }
        }
        .boxed()
    }

    fn capabilities(&self) -> LlmCapabilities {
        self.inner.capabilities()
    }
}

const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

#[derive(Debug, Default)]
pub(crate) struct ThinkingFilter {
    pending: String,
    in_think: bool,
}

impl ThinkingFilter {
    pub(crate) fn push(&mut self, text: &str) -> String {
        self.pending.push_str(text);
        let mut out = String::new();

        loop {
            if self.in_think {
                let Some(close_pos) = find_ascii_case_insensitive(&self.pending, THINK_CLOSE)
                else {
                    let keep = partial_tag_suffix_len(&self.pending, THINK_CLOSE);
                    if keep > 0 {
                        self.pending = self.pending[self.pending.len() - keep..].to_string();
                    } else {
                        self.pending.clear();
                    }
                    return out;
                };
                let drain_to = close_pos + THINK_CLOSE.len();
                self.pending.drain(..drain_to);
                self.in_think = false;
                continue;
            }

            let Some(open_pos) = find_ascii_case_insensitive(&self.pending, THINK_OPEN) else {
                let keep = partial_tag_suffix_len(&self.pending, THINK_OPEN);
                let emit_len = self.pending.len().saturating_sub(keep);
                out.push_str(&self.pending[..emit_len]);
                self.pending.drain(..emit_len);
                return out;
            };

            out.push_str(&self.pending[..open_pos]);
            let drain_to = open_pos + THINK_OPEN.len();
            self.pending.drain(..drain_to);
            self.in_think = true;
        }
    }

    pub(crate) fn finish(&mut self) -> String {
        if self.in_think {
            self.pending.clear();
            self.in_think = false;
            String::new()
        } else {
            std::mem::take(&mut self.pending)
        }
    }
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn partial_tag_suffix_len(value: &str, tag: &str) -> usize {
    let max = value.len().min(tag.len().saturating_sub(1));
    let bytes = value.as_bytes();
    for len in (1..=max).rev() {
        let start = value.len() - len;
        if value.is_char_boundary(start)
            && tag.as_bytes()[..len].eq_ignore_ascii_case(&bytes[start..])
        {
            return len;
        }
    }
    0
}

pub(crate) fn filter_thinking_stream<S>(
    stream: S,
) -> BoxStream<'static, Result<LlmDelta, ProviderError>>
where
    S: futures_util::Stream<Item = Result<LlmDelta, ProviderError>> + Send + 'static,
{
    use futures_util::StreamExt;
    async_stream::try_stream! {
        let mut filter = ThinkingFilter::default();
        futures_util::pin_mut!(stream);
        while let Some(delta) = stream.next().await {
            let text = filter.push(&delta?.text);
            if !text.is_empty() {
                yield LlmDelta { text };
            }
        }
        let text = filter.finish();
        if !text.is_empty() {
            yield LlmDelta { text };
        }
    }
    .boxed()
}

/// 便捷：收齐全部 delta 拼成完整文本（整理/翻译用——注入是一次性的，02 F-2）。
pub async fn collect_text(
    provider: &dyn LlmProvider,
    req: LlmRequest,
) -> Result<String, ProviderError> {
    use futures_util::StreamExt;
    let mut stream = provider.complete(req);
    let mut out = String::new();
    while let Some(delta) = stream.next().await {
        out.push_str(&delta?.text);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use futures_util::stream;

    struct FakeLlm {
        stream: std::sync::Mutex<Option<BoxStream<'static, Result<LlmDelta, ProviderError>>>>,
    }

    impl FakeLlm {
        fn new(stream: BoxStream<'static, Result<LlmDelta, ProviderError>>) -> Self {
            Self {
                stream: std::sync::Mutex::new(Some(stream)),
            }
        }
    }

    impl LlmProvider for FakeLlm {
        fn complete(
            &self,
            _req: LlmRequest,
        ) -> BoxStream<'static, Result<LlmDelta, ProviderError>> {
            self.stream.lock().unwrap().take().unwrap()
        }

        fn capabilities(&self) -> LlmCapabilities {
            LlmCapabilities { streaming: true }
        }
    }

    fn request() -> LlmRequest {
        LlmRequest {
            system: String::new(),
            messages: vec![],
            temperature: 0.0,
            max_tokens: None,
        }
    }

    #[tokio::test]
    async fn profile_timeout_covers_the_complete_stream() {
        let partial = stream::once(async {
            Ok(LlmDelta {
                text: "partial".into(),
            })
        });
        let never_finishes = partial.chain(stream::pending()).boxed();
        let provider = TimedLlmProvider::new(
            Arc::new(FakeLlm::new(never_finishes)),
            Duration::from_millis(1),
        );
        let mut stream = provider.complete(request());

        assert_eq!(stream.next().await.unwrap().unwrap().text, "partial");
        assert!(matches!(
            stream.next().await,
            Some(Err(ProviderError::Timeout))
        ));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn profile_timeout_allows_a_completed_stream() {
        let completed = stream::iter([Ok(LlmDelta {
            text: "done".into(),
        })])
        .boxed();
        let provider =
            TimedLlmProvider::new(Arc::new(FakeLlm::new(completed)), Duration::from_secs(60));

        assert_eq!(collect_text(&provider, request()).await.unwrap(), "done");
    }

    #[test]
    fn thinking_filter_strips_complete_block() {
        let mut f = ThinkingFilter::default();
        assert_eq!(f.push("<think>内部推理</think>最终回答"), "最终回答");
        assert_eq!(f.finish(), "");
    }

    #[test]
    fn thinking_filter_handles_tags_split_across_chunks() {
        let mut f = ThinkingFilter::default();
        assert_eq!(f.push("<thi"), "");
        assert_eq!(f.push("nk>内部"), "");
        assert_eq!(f.push("推理</thi"), "");
        assert_eq!(f.push("nk>答案"), "答案");
        assert_eq!(f.finish(), "");
    }

    #[test]
    fn thinking_filter_preserves_plain_text_with_partial_lookalike() {
        let mut f = ThinkingFilter::default();
        assert_eq!(f.push("这里有 <thing> 标签"), "这里有 <thing> 标签");
        assert_eq!(f.finish(), "");
    }

    #[test]
    fn thinking_filter_handles_utf8_before_partial_tag() {
        let mut f = ThinkingFilter::default();
        assert_eq!(f.push("你好<thi"), "你好");
        assert_eq!(f.push("nk>内部</think>回答"), "回答");
        assert_eq!(f.finish(), "");
    }
}
