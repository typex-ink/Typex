//! OpenAI Chat Completions adapter（03 §3.1）。
//! 覆盖 OpenAI / DeepSeek / Groq / SiliconFlow / OpenRouter / Ollama / 火山方舟。

use super::{LlmCapabilities, LlmDelta, LlmProvider, LlmRequest, filter_thinking_stream};
use crate::providers::ProviderError;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use std::collections::HashMap;

pub struct ChatCompletionsLlm {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    extra_headers: HashMap<String, String>,
    enable_thinking: Option<bool>,
}

impl ChatCompletionsLlm {
    pub fn new(
        client: reqwest::Client,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            extra_headers: HashMap::new(),
            enable_thinking: None,
        }
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.extra_headers = headers;
        self
    }

    pub fn with_thinking(mut self, enable_thinking: Option<bool>) -> Self {
        self.enable_thinking = enable_thinking;
        self
    }

    fn build_body(&self, req: &LlmRequest) -> serde_json::Value {
        let mut messages = vec![serde_json::json!({"role": "system", "content": req.system})];
        for m in &req.messages {
            messages.push(serde_json::json!({"role": m.role, "content": m.content}));
        }
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "temperature": req.temperature,
        });
        if let Some(mt) = req.max_tokens {
            body["max_tokens"] = mt.into();
        }
        if let Some(enable) = self.enable_thinking {
            body["enable_thinking"] = enable.into();
        }
        body
    }
}

/// 解析一行 SSE data JSON → delta 文本。
fn parse_delta(data: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    v["choices"][0]["delta"]["content"]
        .as_str()
        .map(String::from)
}

impl LlmProvider for ChatCompletionsLlm {
    fn complete(&self, req: LlmRequest) -> BoxStream<'static, Result<LlmDelta, ProviderError>> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = self.build_body(&req);
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let extra_headers = self.extra_headers.clone();

        let stream = async_stream_impl(client, url, api_key, extra_headers, body);
        filter_thinking_stream(stream)
    }

    fn capabilities(&self) -> LlmCapabilities {
        LlmCapabilities { streaming: true }
    }
}

fn async_stream_impl(
    client: reqwest::Client,
    url: String,
    api_key: String,
    extra_headers: HashMap<String, String>,
    body: serde_json::Value,
) -> impl futures_util::Stream<Item = Result<LlmDelta, ProviderError>> + Send {
    async_stream::try_stream! {
        let mut req = client.post(&url).bearer_auth(&api_key).json(&body);
        for (k, v) in &extra_headers {
            req = req.header(k, v);
        }
        let resp = req.send().await.map_err(ProviderError::from_reqwest)?;
        let status = resp.status().as_u16();
        let resp = if status >= 400 {
            let text = resp.text().await.unwrap_or_default();
            Err(ProviderError::from_status(status, text))?;
            unreachable!()
        } else {
            resp
        };
        let mut events = resp.bytes_stream().eventsource();
        while let Some(event) = events.next().await {
            let event = event.map_err(|e| ProviderError::Network(format!("SSE 解析失败: {e}")))?;
            if event.data == "[DONE]" {
                break;
            }
            if let Some(text) = parse_delta(&event.data)
                && !text.is_empty() {
                    yield LlmDelta { text };
                }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_delta_extracts_content() {
        let data = r#"{"choices":[{"delta":{"content":"你好"}}]}"#;
        assert_eq!(parse_delta(data), Some("你好".into()));
    }

    #[test]
    fn parse_delta_none_for_role_only_chunk() {
        let data = r#"{"choices":[{"delta":{"role":"assistant"}}]}"#;
        assert_eq!(parse_delta(data), None);
    }

    #[test]
    fn parse_delta_none_for_invalid_json() {
        assert_eq!(parse_delta("not json"), None);
    }
}
