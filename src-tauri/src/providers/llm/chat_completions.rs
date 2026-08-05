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
    reasoning_effort: Option<String>,
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
            reasoning_effort: None,
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

    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> Self {
        self.reasoning_effort = effort;
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
        if let Some(effort) = &self.reasoning_effort {
            body["reasoning_effort"] = effort.clone().into();
        }
        body
    }
}

enum ChatCompletionsEvent {
    Delta(String),
    Failed(String),
    Other,
}

/// 解析一条 SSE 事件；错误事件必须保留完整 data 文本。
fn parse_event(event_type: &str, data: &str) -> ChatCompletionsEvent {
    if event_type == "error" {
        return ChatCompletionsEvent::Failed(data.to_string());
    }
    let value: serde_json::Value = match serde_json::from_str(data) {
        Ok(value) => value,
        Err(_) => return ChatCompletionsEvent::Other,
    };
    if value.get("error").is_some_and(|error| !error.is_null())
        || value.get("type").and_then(|value| value.as_str()) == Some("error")
    {
        return ChatCompletionsEvent::Failed(data.to_string());
    }
    match value["choices"][0]["delta"]["content"].as_str() {
        Some(text) => ChatCompletionsEvent::Delta(text.to_string()),
        None => ChatCompletionsEvent::Other,
    }
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
            match parse_event(&event.event, &event.data) {
                ChatCompletionsEvent::Delta(text) if !text.is_empty() => {
                    yield LlmDelta { text };
                }
                ChatCompletionsEvent::Failed(body) => {
                    Err(ProviderError::from_stream_error(body))?;
                }
                ChatCompletionsEvent::Delta(_) | ChatCompletionsEvent::Other => {}
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
        assert!(matches!(
            parse_event("message", data),
            ChatCompletionsEvent::Delta(text) if text == "你好"
        ));
    }

    #[test]
    fn parse_delta_none_for_role_only_chunk() {
        let data = r#"{"choices":[{"delta":{"role":"assistant"}}]}"#;
        assert!(matches!(
            parse_event("message", data),
            ChatCompletionsEvent::Other
        ));
    }

    #[test]
    fn parse_delta_none_for_invalid_json() {
        assert!(matches!(
            parse_event("message", "not json"),
            ChatCompletionsEvent::Other
        ));
    }

    #[test]
    fn parse_error_event_keeps_complete_data() {
        let data = r#"{"error":{"message":"failed","request_id":"req-123"}}"#;
        assert!(matches!(
            parse_event("message", data),
            ChatCompletionsEvent::Failed(body) if body == data
        ));
        assert!(matches!(
            parse_event("error", "plain failure"),
            ChatCompletionsEvent::Failed(body) if body == "plain failure"
        ));
    }

    #[test]
    fn build_body_includes_reasoning_effort_when_configured() {
        let llm = ChatCompletionsLlm::new(
            reqwest::Client::new(),
            "https://api.example.com/v1",
            "k",
            "m",
        )
        .with_reasoning_effort(Some("high".into()));
        let req = LlmRequest {
            system: "sys".into(),
            messages: vec![],
            temperature: 0.2,
            max_tokens: None,
        };
        let body = llm.build_body(&req);
        assert_eq!(body["reasoning_effort"], "high");
        assert!(body.get("enable_thinking").is_none());
    }
}
