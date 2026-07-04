//! OpenAI Responses adapter（03 §3.2）。
//! SSE 事件：response.output_text.delta / response.completed / response.failed。

use super::{LlmCapabilities, LlmDelta, LlmProvider, LlmRequest};
use crate::providers::ProviderError;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use std::collections::HashMap;

pub struct ResponsesLlm {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    extra_headers: HashMap<String, String>,
}

impl ResponsesLlm {
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
        }
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.extra_headers = headers;
        self
    }

    fn build_body(&self, req: &LlmRequest) -> serde_json::Value {
        let mut input = vec![serde_json::json!({
            "role": "system",
            "content": [{"type": "input_text", "text": req.system}]
        })];
        for m in &req.messages {
            let ctype = if m.role == "assistant" {
                "output_text"
            } else {
                "input_text"
            };
            input.push(serde_json::json!({
                "role": m.role,
                "content": [{"type": ctype, "text": m.content}]
            }));
        }
        let mut body = serde_json::json!({
            "model": self.model,
            "input": input,
            "stream": true,
            "temperature": req.temperature,
        });
        if let Some(mt) = req.max_tokens {
            body["max_output_tokens"] = mt.into();
        }
        body
    }
}

/// SSE 事件解析：返回 (delta 文本 | 结束 | 失败)。
enum ResponsesEvent {
    Delta(String),
    Completed,
    Failed(String),
    Other,
}

fn parse_event(event_type: &str, data: &str) -> ResponsesEvent {
    match event_type {
        "response.output_text.delta" => {
            let v: serde_json::Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => return ResponsesEvent::Other,
            };
            match v["delta"].as_str() {
                Some(t) => ResponsesEvent::Delta(t.to_string()),
                None => ResponsesEvent::Other,
            }
        }
        "response.completed" => ResponsesEvent::Completed,
        "response.failed" | "error" => {
            let v: serde_json::Value = serde_json::from_str(data).unwrap_or_default();
            let msg = v["response"]["error"]["message"]
                .as_str()
                .or_else(|| v["message"].as_str())
                .unwrap_or("响应失败")
                .to_string();
            ResponsesEvent::Failed(msg)
        }
        _ => ResponsesEvent::Other,
    }
}

impl LlmProvider for ResponsesLlm {
    fn complete(&self, req: LlmRequest) -> BoxStream<'static, Result<LlmDelta, ProviderError>> {
        let url = format!("{}/responses", self.base_url);
        let body = self.build_body(&req);
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let extra_headers = self.extra_headers.clone();

        let stream = async_stream::try_stream! {
            let mut request = client.post(&url).bearer_auth(&api_key).json(&body);
            for (k, v) in &extra_headers {
                request = request.header(k, v);
            }
            let resp = request.send().await.map_err(ProviderError::from_reqwest)?;
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
                match parse_event(&event.event, &event.data) {
                    ResponsesEvent::Delta(text) => yield LlmDelta { text },
                    ResponsesEvent::Completed => break,
                    ResponsesEvent::Failed(msg) => {
                        Err(ProviderError::Server { status: 500, body: msg })?;
                    }
                    ResponsesEvent::Other => {}
                }
            }
        };
        stream.boxed()
    }

    fn capabilities(&self) -> LlmCapabilities {
        LlmCapabilities { streaming: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_delta_event() {
        match parse_event("response.output_text.delta", r#"{"delta":"Hi"}"#) {
            ResponsesEvent::Delta(t) => assert_eq!(t, "Hi"),
            _ => panic!("expected Delta"),
        }
    }

    #[test]
    fn parse_completed_and_failed() {
        assert!(matches!(
            parse_event("response.completed", "{}"),
            ResponsesEvent::Completed
        ));
        match parse_event(
            "response.failed",
            r#"{"response":{"error":{"message":"boom"}}}"#,
        ) {
            ResponsesEvent::Failed(m) => assert_eq!(m, "boom"),
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn unknown_events_ignored() {
        assert!(matches!(
            parse_event("response.output_item.added", "{}"),
            ResponsesEvent::Other
        ));
    }
}
