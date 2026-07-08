//! Provider × wiremock 集成测试（07 §4.1）。

use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use typex_lib::orchestrator::assistant::{AssistantOutcome, AssistantService};
use typex_lib::orchestrator::pipeline::{self, ProcessOutcome};
use typex_lib::providers::ProviderError;
use typex_lib::providers::ProviderRegistry;
use typex_lib::providers::llm::{
    LlmProvider, LlmRequest, chat_completions::ChatCompletionsLlm, responses::ResponsesLlm,
};
use typex_lib::providers::stt::{
    AudioInput, SttOptions, SttProvider, openai_compat::OpenAiCompatStt,
};
use typex_lib::settings::SettingsService;
use typex_lib::settings::schema::{Settings, SlotConfig};
use typex_lib::types::{ProviderCapability, ProviderKind, ProviderProfile, SessionMode, SlotKind};
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

fn test_llm_profile(id: &str, base_url: &str) -> ProviderProfile {
    ProviderProfile {
        id: id.into(),
        capability: ProviderCapability::Llm,
        kind: ProviderKind::ChatCompletions,
        label: id.into(),
        base_url: base_url.into(),
        model: "test-model".into(),
        credentials: [("api_key".to_string(), "sk-llm".to_string())].into(),
        extra_headers: HashMap::new(),
        extra_form: HashMap::new(),
        timeout_ms: 5_000,
        options: HashMap::new(),
    }
}

#[tokio::test]
async fn dictation_polish_receives_target_app_context() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |req: &Request| {
            let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            let content = v["messages"][1]["content"].as_str().unwrap();
            assert!(content.contains("<transcript>raw text</transcript>"));
            assert!(content.contains("<target_app>Terminal</target_app>"));
            assert!(content.contains("<dictionary>- Typex\n- OpenAI</dictionary>"));
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body(&["polished text"]))
        })
        .expect(1)
        .mount(&server)
        .await;

    let mut settings = Settings::default();
    settings.dictation.polish_enabled = true;
    settings.dictionary.terms = vec!["Typex".into(), "OpenAI".into()];
    settings.profiles = vec![test_llm_profile("polish", &server.uri())];
    settings.slots.insert(
        SlotKind::Polish,
        SlotConfig {
            active_profile: Some("polish".into()),
        },
    );
    let registry = Arc::new(ProviderRegistry::new(settings.clone()));
    let prompt_context = pipeline::PromptContext::new(Some("Terminal".into()));

    let out = pipeline::process(
        SessionMode::Dictation,
        "raw text".into(),
        &settings,
        &registry,
        &prompt_context,
    )
    .await;

    match out {
        ProcessOutcome::Done(text) => assert_eq!(text, "polished text"),
        _ => panic!("expected polished output"),
    }
}

#[tokio::test]
async fn translation_uses_polished_transcript_when_enabled() {
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_mock = calls.clone();
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |req: &Request| {
            let idx = calls_for_mock.fetch_add(1, Ordering::SeqCst);
            let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            let content = v["messages"][1]["content"].as_str().unwrap();
            match idx {
                0 => {
                    assert!(content.contains("<transcript>嗯 raw</transcript>"));
                    assert!(content.contains("<target_app>Slack</target_app>"));
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string(sse_body(&["clean"]))
                }
                1 => {
                    assert!(content.contains("<text>clean</text>"));
                    assert!(content.contains("<target_app>Slack</target_app>"));
                    assert!(!content.contains("嗯 raw"));
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string(sse_body(&["translated"]))
                }
                _ => panic!("unexpected LLM call"),
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let mut settings = Settings::default();
    settings.dictation.polish_enabled = true;
    settings.profiles = vec![
        test_llm_profile("polish", &server.uri()),
        test_llm_profile("translate", &server.uri()),
    ];
    settings.slots.insert(
        SlotKind::Polish,
        SlotConfig {
            active_profile: Some("polish".into()),
        },
    );
    settings.slots.insert(
        SlotKind::Translate,
        SlotConfig {
            active_profile: Some("translate".into()),
        },
    );
    let registry = Arc::new(ProviderRegistry::new(settings.clone()));
    let prompt_context = pipeline::PromptContext::new(Some("Slack".into()));

    let out = pipeline::process(
        SessionMode::Translation,
        "嗯 raw".into(),
        &settings,
        &registry,
        &prompt_context,
    )
    .await;

    match out {
        ProcessOutcome::Done(text) => assert_eq!(text, "translated"),
        _ => panic!("expected translated output"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn translation_skips_polish_when_disabled() {
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_mock = calls.clone();
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |req: &Request| {
            calls_for_mock.fetch_add(1, Ordering::SeqCst);
            let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            let content = v["messages"][1]["content"].as_str().unwrap();
            assert!(content.contains("<text>raw</text>"));
            assert!(content.contains("<target_app>Mail</target_app>"));
            assert!(!content.contains("<transcript>raw</transcript>"));
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body(&["translated"]))
        })
        .expect(1)
        .mount(&server)
        .await;

    let mut settings = Settings::default();
    settings.dictation.polish_enabled = false;
    settings.profiles = vec![test_llm_profile("translate", &server.uri())];
    settings.slots.insert(
        SlotKind::Translate,
        SlotConfig {
            active_profile: Some("translate".into()),
        },
    );
    let registry = Arc::new(ProviderRegistry::new(settings.clone()));
    let prompt_context = pipeline::PromptContext::new(Some("Mail".into()));

    let out = pipeline::process(
        SessionMode::Translation,
        "raw".into(),
        &settings,
        &registry,
        &prompt_context,
    )
    .await;

    match out {
        ProcessOutcome::Done(text) => assert_eq!(text, "translated"),
        _ => panic!("expected translated output"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn assistant_uses_polished_instruction_when_enabled() {
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_mock = calls.clone();
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |req: &Request| {
            let idx = calls_for_mock.fetch_add(1, Ordering::SeqCst);
            let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            let content = v["messages"][1]["content"].as_str().unwrap();
            match idx {
                0 => {
                    assert!(content.contains("<transcript>呃 explain</transcript>"));
                    assert!(content.contains("<target_app>VS Code</target_app>"));
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string(sse_body(&["explain"]))
                }
                1 => {
                    assert!(content.contains("<question>explain</question>"));
                    assert!(content.contains("<target_app>VS Code</target_app>"));
                    assert!(!content.contains("呃 explain"));
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string(sse_body(&["answer"]))
                }
                _ => panic!("unexpected LLM call"),
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let mut settings = Settings::default();
    settings.dictation.polish_enabled = true;
    settings.profiles = vec![
        test_llm_profile("polish", &server.uri()),
        test_llm_profile("assistant", &server.uri()),
    ];
    settings.slots.insert(
        SlotKind::Polish,
        SlotConfig {
            active_profile: Some("polish".into()),
        },
    );
    settings.slots.insert(
        SlotKind::Assistant,
        SlotConfig {
            active_profile: Some("assistant".into()),
        },
    );

    let dir = tempfile::tempdir().unwrap();
    let settings_service = Arc::new(SettingsService::load(dir.path().to_path_buf()));
    settings_service.update(settings.clone()).unwrap();
    let registry = Arc::new(ProviderRegistry::new(settings));
    let assistant = AssistantService::new(
        settings_service,
        registry,
        Box::new(|_| {}),
        Box::new(|_| Box::pin(async {})),
    );

    let out = assistant
        .run(
            "呃 explain".into(),
            None,
            false,
            pipeline::PromptContext::new(Some("VS Code".into())),
        )
        .await
        .unwrap();

    assert_eq!(out, AssistantOutcome::HandedOff);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
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
async fn chat_completions_sends_thinking_option_and_strips_think_blocks() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |req: &Request| {
            let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(v["enable_thinking"], false);
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body(&["<thi", "nk>内部推理", "</think>你好"]))
        })
        .mount(&server)
        .await;

    let llm = ChatCompletionsLlm::new(client(), server.uri(), "sk-llm", "test-model")
        .with_thinking(Some(false));
    let mut out = String::new();
    let mut stream = llm.complete(llm_req());
    while let Some(d) = stream.next().await {
        out.push_str(&d.unwrap().text);
    }
    assert_eq!(out, "你好");
}

#[tokio::test]
async fn chat_completions_sends_reasoning_effort_when_configured() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |req: &Request| {
            let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(v["reasoning_effort"], "high");
            assert!(v.get("enable_thinking").is_none());
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body(&["ok"]))
        })
        .mount(&server)
        .await;

    let llm = ChatCompletionsLlm::new(client(), server.uri(), "sk-llm", "test-model")
        .with_reasoning_effort(Some("high".into()));
    let mut stream = llm.complete(llm_req());
    while let Some(d) = stream.next().await {
        d.unwrap();
    }
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
async fn responses_sends_reasoning_effort_when_configured() {
    let server = MockServer::start().await;
    let body = "event: response.output_text.delta\ndata: {\"delta\":\"ok\"}\n\n\
                event: response.completed\ndata: {}\n\n";
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(move |req: &Request| {
            let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(v["reasoning"]["effort"], "high");
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body)
        })
        .mount(&server)
        .await;

    let llm = ResponsesLlm::new(client(), server.uri(), "k", "gpt-test")
        .with_reasoning_effort(Some("high".into()));
    let mut stream = llm.complete(llm_req());
    while let Some(d) = stream.next().await {
        d.unwrap();
    }
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
            assert_eq!(body["request"]["corpus"]["context"], "Typex\nOpenAI");
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
        .transcribe(
            wav_stub(),
            SttOptions {
                prompt: Some("Typex\nOpenAI".into()),
                ..Default::default()
            },
        )
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
