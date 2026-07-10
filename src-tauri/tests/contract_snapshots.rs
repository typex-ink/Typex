//! 契约快照测试（07 §4.1）：四个 adapter 构造的完整 HTTP 请求形状。
//!
//! 厂商格式是外部契约——任何无意的请求变化（哪怕是「顺手重构」）都会在快照 diff 中显形。
//! 波动字段（multipart boundary、request-id、host）先归一化再快照。
//! HTTP 客户端默认 user-agent 不是 Provider 协议契约，且不同环境可能缺省不同。

use futures_util::StreamExt;
use typex_lib::providers::llm::{
    LlmProvider, LlmRequest, Msg, chat_completions::ChatCompletionsLlm, responses::ResponsesLlm,
};
use typex_lib::providers::stt::{
    AudioInput, SttOptions, SttProvider, openai_compat::OpenAiCompatStt, volcengine::VolcengineStt,
};
use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::method};

/// 捕获到的请求 → 归一化 JSON（快照对象）。
fn normalize(req: &Request) -> serde_json::Value {
    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter_map(|(k, v)| {
            let name = k.as_str().to_lowercase();
            if name == "user-agent" {
                return None;
            }
            let mut val = v.to_str().unwrap_or("<binary>").to_string();
            // 归一化波动值
            if name == "host" {
                val = "<host>".into();
            }
            if name == "content-length" {
                val = "<len>".into();
            }
            if name == "x-api-request-id" {
                val = "<uuid>".into();
            }
            if name == "content-type" && val.contains("boundary=") {
                val = val.split("boundary=").next().unwrap_or("").to_string() + "boundary=<b>";
            }
            Some((name, val))
        })
        .collect();
    headers.sort();

    let body = String::from_utf8_lossy(&req.body).to_string();
    let body = normalize_body(&body);

    serde_json::json!({
        "method": req.method.to_string(),
        "path": req.url.path(),
        "headers": headers.into_iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>(),
        "body": body,
    })
}

fn normalize_body(body: &str) -> serde_json::Value {
    // JSON body → 原样嵌入（键序由 serde 保持）
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        return v;
    }
    // multipart → 替换 boundary 行，保留字段结构
    let mut lines: Vec<String> = Vec::new();
    for line in body.lines() {
        if line.starts_with("--") {
            lines.push("--<boundary>".into());
        } else if line.chars().any(|c| c.is_control() && c != '\r') {
            lines.push("<binary>".into());
        } else {
            lines.push(line.trim_end_matches('\r').to_string());
        }
    }
    serde_json::Value::String(lines.join("\n"))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap()
}

fn wav_stub() -> AudioInput {
    AudioInput {
        wav_16k_mono: b"RIFFwavstub!".to_vec(),
        duration_ms: 500,
    }
}

/// 起一个捕获请求的 mock server：返回 (server, 捕获槽)。
async fn capture_server(
    response: ResponseTemplate,
) -> (
    MockServer,
    std::sync::Arc<std::sync::Mutex<Option<serde_json::Value>>>,
) {
    let server = MockServer::start().await;
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot2 = slot.clone();
    Mock::given(method("POST"))
        .respond_with(move |req: &Request| {
            *slot2.lock().unwrap() = Some(normalize(req));
            response.clone()
        })
        .mount(&server)
        .await;
    (server, slot)
}

#[tokio::test]
async fn snapshot_openai_compat_stt_request() {
    let (server, slot) =
        capture_server(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "ok"})))
            .await;
    let stt = OpenAiCompatStt::new(
        client(),
        format!("{}/v1", server.uri()),
        "sk-test",
        "whisper-1",
    )
    .with_extras(
        [("X-Custom".to_string(), "1".to_string())].into(),
        [("vad_filter".to_string(), "true".to_string())].into(),
    );
    let opts = SttOptions {
        language: Some("zh".into()),
        prompt: Some("Typex".into()),
        temperature: Some(0.0),
    };
    stt.transcribe(wav_stub(), opts).await.unwrap();
    let captured = slot.lock().unwrap().clone().unwrap();
    insta::assert_json_snapshot!("openai_compat_stt_request", captured);
}

#[tokio::test]
async fn snapshot_volcengine_stt_request() {
    let (server, slot) = capture_server(
        ResponseTemplate::new(200)
            .insert_header("X-Api-Status-Code", "20000000")
            .set_body_json(serde_json::json!({"result": {"text": "ok"}})),
    )
    .await;
    let stt = VolcengineStt::new(client(), server.uri(), "app-key-1", "access-token-1");
    stt.transcribe(wav_stub(), SttOptions::default())
        .await
        .unwrap();
    let captured = slot.lock().unwrap().clone().unwrap();
    insta::assert_json_snapshot!("volcengine_stt_request", captured);
}

fn llm_req() -> LlmRequest {
    LlmRequest {
        system: "系统提示".into(),
        messages: vec![Msg {
            role: "user".into(),
            content: "你好".into(),
        }],
        temperature: 0.3,
        max_tokens: Some(256),
    }
}

#[tokio::test]
async fn snapshot_chat_completions_request() {
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";
    let (server, slot) = capture_server(
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(sse),
    )
    .await;
    let llm = ChatCompletionsLlm::new(client(), format!("{}/v1", server.uri()), "sk-test", "gpt-x")
        .with_headers([("X-Org".to_string(), "typex".to_string())].into());
    let mut stream = llm.complete(llm_req());
    while stream.next().await.is_some() {}
    let captured = slot.lock().unwrap().clone().unwrap();
    insta::assert_json_snapshot!("chat_completions_request", captured);
}

#[tokio::test]
async fn snapshot_responses_request() {
    let sse = "event: response.output_text.delta\ndata: {\"delta\":\"ok\"}\n\n\
               event: response.completed\ndata: {}\n\n";
    let (server, slot) = capture_server(
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(sse),
    )
    .await;
    let llm = ResponsesLlm::new(client(), format!("{}/v1", server.uri()), "sk-test", "gpt-x");
    let mut stream = llm.complete(llm_req());
    while stream.next().await.is_some() {}
    let captured = slot.lock().unwrap().clone().unwrap();
    insta::assert_json_snapshot!("responses_request", captured);
}
