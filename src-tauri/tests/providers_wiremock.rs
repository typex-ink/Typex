//! Provider × wiremock 集成测试（08 §4.1）。

use futures_util::StreamExt;
use typex_lib::providers::ProviderError;
use typex_lib::providers::llm::{
    LlmProvider, LlmRequest, chat_completions::ChatCompletionsLlm, responses::ResponsesLlm,
};
use typex_lib::providers::stt::{
    AudioInput, SttOptions, SttProvider, openai_compat::OpenAiCompatStt,
};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn wav_stub() -> AudioInput {
    AudioInput {
        wav_16k_mono: vec![0u8; 128],
        duration_ms: 1000,
    }
}

fn llm_req() -> LlmRequest {
    LlmRequest {
        system: "sys".into(),
        messages: vec![typex_lib::providers::llm::Msg {
            role: "user".into(),
            content: "ping".into(),
        }],
        temperature: 0.3,
        max_tokens: None,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap()
}

// ── openai_compat STT ──

#[tokio::test]
async fn stt_request_shape_and_response_parse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .and(header("authorization", "Bearer sk-test"))
        .respond_with(move |req: &Request| {
            let body = String::from_utf8_lossy(&req.body);
            // multipart 字段完整性
            assert!(body.contains("name=\"file\""), "缺 file 字段");
            assert!(body.contains("name=\"model\""), "缺 model 字段");
            assert!(body.contains("whisper-large-v3-turbo"));
            assert!(body.contains("name=\"language\""));
            assert!(body.contains("name=\"response_format\""));
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "你好 Typex"}))
        })
        .mount(&server)
        .await;

    // base_url 带尾斜杠也能正确拼接
    let stt = OpenAiCompatStt::new(
        client(),
        format!("{}/v1/", server.uri()),
        "sk-test",
        "whisper-large-v3-turbo",
    );
    let t = stt
        .transcribe(
            wav_stub(),
            SttOptions {
                language: Some("zh".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(t.text, "你好 Typex");
}

#[tokio::test]
async fn stt_language_auto_is_omitted() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(move |req: &Request| {
            let body = String::from_utf8_lossy(&req.body);
            assert!(
                !body.contains("name=\"language\""),
                "auto 不应发送 language"
            );
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "ok"}))
        })
        .mount(&server)
        .await;
    let stt = OpenAiCompatStt::new(client(), server.uri(), "k", "m");
    stt.transcribe(
        wav_stub(),
        SttOptions {
            language: Some("auto".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn stt_qwen_asr_envelope_from_compat_response_is_stripped() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text": "language Chinese<asr_text>你好。"
        })))
        .mount(&server)
        .await;

    let stt = OpenAiCompatStt::new(client(), server.uri(), "k", "qwen3-asr");
    let t = stt
        .transcribe(wav_stub(), SttOptions::default())
        .await
        .unwrap();
    assert_eq!(t.text, "你好。");
    assert_eq!(t.detected_language.as_deref(), Some("Chinese"));
}

#[tokio::test]
async fn stt_401_is_auth_error_and_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
        .expect(1) // 不重试
        .mount(&server)
        .await;
    let stt = OpenAiCompatStt::new(client(), server.uri(), "bad", "m");
    let err = stt
        .transcribe(wav_stub(), SttOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Auth(_)));
}

#[tokio::test]
async fn stt_503_retried_twice_then_gives_up() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .expect(3) // 1 + 2 重试；multipart body 在重试间可重放
        .mount(&server)
        .await;
    let stt = OpenAiCompatStt::new(client(), server.uri(), "k", "m");
    let err = stt
        .transcribe(wav_stub(), SttOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Server { status: 503, .. }));
}

#[tokio::test]
async fn stt_timeout_classified() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"text": "slow"}))
                .set_delay(std::time::Duration::from_secs(10)),
        )
        .mount(&server)
        .await;
    let fast_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(300))
        .build()
        .unwrap();
    let stt = OpenAiCompatStt::new(fast_client, server.uri(), "k", "m");
    let err = stt
        .transcribe(wav_stub(), SttOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Timeout));
}

#[tokio::test]
async fn stt_extra_headers_and_form_passthrough() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("x-gateway-key", "gw"))
        .respond_with(move |req: &Request| {
            let body = String::from_utf8_lossy(&req.body);
            assert!(body.contains("name=\"custom_field\""));
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "ok"}))
        })
        .mount(&server)
        .await;
    let stt = OpenAiCompatStt::new(client(), server.uri(), "k", "m").with_extras(
        [("x-gateway-key".to_string(), "gw".to_string())].into(),
        [("custom_field".to_string(), "v".to_string())].into(),
    );
    stt.transcribe(wav_stub(), SttOptions::default())
        .await
        .unwrap();
}

// ── chat_completions LLM ──

fn sse_body(chunks: &[&str]) -> String {
    let mut s = String::new();
    for c in chunks {
        s.push_str(&format!(
            "data: {}\n\n",
            serde_json::json!({"choices":[{"delta":{"content":c}}]})
        ));
    }
    s.push_str("data: [DONE]\n\n");
    s
}

#[tokio::test]
async fn chat_completions_streams_deltas() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer sk-llm"))
        .respond_with(move |req: &Request| {
            let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(v["model"], "test-model");
            assert_eq!(v["stream"], true);
            assert_eq!(v["messages"][0]["role"], "system");
            assert_eq!(v["messages"][1]["content"], "ping");
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body(&["你", "好", "！"]))
        })
        .mount(&server)
        .await;

    let llm = ChatCompletionsLlm::new(client(), server.uri(), "sk-llm", "test-model");
    let mut out = String::new();
    let mut stream = llm.complete(llm_req());
    while let Some(d) = stream.next().await {
        out.push_str(&d.unwrap().text);
    }
    assert_eq!(out, "你好！");
}

#[tokio::test]
async fn chat_completions_error_status_maps() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
        .mount(&server)
        .await;
    let llm = ChatCompletionsLlm::new(client(), server.uri(), "k", "m");
    let mut stream = llm.complete(llm_req());
    let first = stream.next().await.unwrap();
    assert!(matches!(first.unwrap_err(), ProviderError::RateLimited(_)));
}

// ── responses LLM ──

#[tokio::test]
async fn responses_streams_output_text_delta() {
    let server = MockServer::start().await;
    let body = "event: response.output_text.delta\ndata: {\"delta\":\"Hel\"}\n\n\
                event: response.output_text.delta\ndata: {\"delta\":\"lo\"}\n\n\
                event: response.completed\ndata: {}\n\n";
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(move |req: &Request| {
            let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert!(v["input"].is_array());
            assert_eq!(v["input"][0]["role"], "system");
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body)
        })
        .mount(&server)
        .await;

    let llm = ResponsesLlm::new(client(), server.uri(), "k", "gpt-test");
    let mut out = String::new();
    let mut stream = llm.complete(llm_req());
    while let Some(d) = stream.next().await {
        out.push_str(&d.unwrap().text);
    }
    assert_eq!(out, "Hello");
}

#[tokio::test]
async fn responses_failed_event_maps_to_error() {
    let server = MockServer::start().await;
    let body = "event: response.output_text.delta\ndata: {\"delta\":\"部分\"}\n\n\
                event: response.failed\ndata: {\"response\":{\"error\":{\"message\":\"overloaded\"}}}\n\n";
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let llm = ResponsesLlm::new(client(), server.uri(), "k", "m");
    let mut stream = llm.complete(llm_req());
    let mut got_delta = false;
    let mut got_err = false;
    while let Some(item) = stream.next().await {
        match item {
            Ok(d) => {
                assert_eq!(d.text, "部分");
                got_delta = true;
            }
            Err(e) => {
                assert!(matches!(e, ProviderError::Server { .. }));
                got_err = true;
                break;
            }
        }
    }
    assert!(got_delta && got_err);
}

// ── volcengine STT（03 §2.2）──

use typex_lib::providers::stt::volcengine::VolcengineStt;

#[tokio::test]
async fn volc_request_shape_and_response_parse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("X-Api-App-Key", "app-1"))
        .and(header("X-Api-Access-Key", "tok-1"))
        .and(header("X-Api-Resource-Id", "volc.bigasr.auc_turbo"))
        .respond_with(move |req: &Request| {
            // 双凭据 header + base64 JSON body（03 §2.2）
            assert!(
                req.headers.get("X-Api-Request-Id").is_some(),
                "缺 Request-Id"
            );
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["user"]["uid"], "typex");
            assert_eq!(body["audio"]["format"], "wav");
            assert_eq!(body["request"]["model_name"], "bigmodel");
            assert_eq!(body["request"]["enable_punc"], true);
            // 音频是合法 base64
            use base64::Engine;
            let data = body["audio"]["data"].as_str().unwrap();
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap();
            assert_eq!(decoded.len(), 128);
            ResponseTemplate::new(200)
                .insert_header("X-Api-Status-Code", "20000000")
                .set_body_json(serde_json::json!({"result": {"text": "火山转写结果"}}))
        })
        .mount(&server)
        .await;

    let stt = VolcengineStt::new(client(), server.uri(), "app-1", "tok-1");
    let t = stt
        .transcribe(wav_stub(), SttOptions::default())
        .await
        .unwrap();
    assert_eq!(t.text, "火山转写结果");
}

#[tokio::test]
async fn volc_error_status_header_maps_auth_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Api-Status-Code", "45000030")
                .set_body_string("invalid access token"),
        )
        .expect(1) // Auth 不重试
        .mount(&server)
        .await;

    let stt = VolcengineStt::new(client(), server.uri(), "a", "t");
    let err = stt
        .transcribe(wav_stub(), SttOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Auth(_)), "{err:?}");
}

#[tokio::test]
async fn volc_server_status_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Api-Status-Code", "55000001")
                .set_body_string("internal"),
        )
        .expect(3) // 首次 + 重试 2 次
        .mount(&server)
        .await;

    let stt = VolcengineStt::new(client(), server.uri(), "a", "t");
    let err = stt
        .transcribe(wav_stub(), SttOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Server { .. }), "{err:?}");
}

#[tokio::test]
async fn volc_missing_status_header_falls_back_to_http_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let stt = VolcengineStt::new(client(), server.uri(), "a", "t");
    let err = stt
        .transcribe(wav_stub(), SttOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Auth(_)), "{err:?}");
}

#[tokio::test]
async fn volc_custom_resource_id_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("X-Api-Resource-Id", "volc.bigasr.custom"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Api-Status-Code", "20000000")
                .set_body_json(serde_json::json!({"result": {"text": "ok"}})),
        )
        .mount(&server)
        .await;

    let stt =
        VolcengineStt::new(client(), server.uri(), "a", "t").with_resource_id("volc.bigasr.custom");
    let t = stt
        .transcribe(wav_stub(), SttOptions::default())
        .await
        .unwrap();
    assert_eq!(t.text, "ok");
}
