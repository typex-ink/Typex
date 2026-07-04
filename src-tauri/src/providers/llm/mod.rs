//! LlmProvider trait（03 §3）。adapter 实现在 CP-1.4：chat_completions / responses。

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
