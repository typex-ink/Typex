//! LlmProvider trait + PromptKit（03 §3）。
pub mod chat_completions;
pub mod prompt;
pub mod responses;

use super::ProviderError;
use futures_util::stream::BoxStream;

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
