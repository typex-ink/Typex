//! 模型下载管理器集成测试（03 §8）。
//!
//! 仅在 feature = local-models 时编译。
//! 使用 wiremock 模拟 HuggingFace / ModelScope 两源；临时目录作 data_dir 注入。

#![cfg(feature = "local-models")]

use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use typex_lib::local::download::{
    DownloadError, Progress, ProgressFn, delete_model, download_model_file, list_downloaded,
    sha256_bytes,
};
use typex_lib::local::manifest::{ModelEngine, ModelEntry, ModelFile, ModelPurpose, ModelSource};
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{method, path},
};

// ── 辅助 ──────────────────────────────────────────────────────────────────────

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap()
}

/// 构造含单文件的测试 ModelEntry，指向指定 base_url。
fn make_entry(hf_base: &str, ms_base: &str, sha256: &str, bytes: u64) -> ModelEntry {
    ModelEntry {
        id: "test-model".into(),
        display_name: "Test Model".into(),
        purpose: ModelPurpose::Stt,
        engine: ModelEngine::Sherpa,
        files: vec![ModelFile {
            name: "model.bin".into(),
            bytes,
            sha256: sha256.to_string(),
        }],
        license: "Apache-2.0".into(),
        sources: vec![
            ModelSource {
                id: "huggingface".into(),
                label: "HuggingFace".into(),
                url_prefix: hf_base.to_string(),
            },
            ModelSource {
                id: "modelscope".into(),
                label: "ModelScope".into(),
                url_prefix: ms_base.to_string(),
            },
        ],
        min_ram_gb: 2,
        requires_gpu: false,
    }
}

// ── 测试 1：Range 请求形状 ─────────────────────────────────────────────────────

/// 验证 download_model_file 在续传场景正确发送 `Range: bytes=N-` 头。
#[tokio::test]
async fn range_request_sent_when_part_file_exists() {
    let server = MockServer::start().await;
    let captured_range = Arc::new(Mutex::new(None::<String>));
    let captured_range2 = captured_range.clone();

    Mock::given(method("GET"))
        .and(path("/model.bin"))
        .respond_with(move |req: &Request| {
            let range = req
                .headers
                .get("range")
                .map(|v| v.to_str().unwrap_or("").to_string());
            *captured_range2.lock().unwrap() = range;
            // 返回 206 Partial Content，剩余字节
            ResponseTemplate::new(206)
                .insert_header("Content-Range", "bytes 5-9/10")
                .set_body_bytes(b"world".to_vec())
        })
        .mount(&server)
        .await;

    let data_dir = tempdir().unwrap();
    let model_dir = data_dir.path().join("models").join("test-model");
    tokio::fs::create_dir_all(&model_dir).await.unwrap();

    // 预先写入 .part 文件（模拟已下载 5 字节）
    let part_path = model_dir.join("model.bin.part");
    tokio::fs::write(&part_path, b"hello").await.unwrap();

    let body_full = b"helloworld";
    let sha = sha256_bytes(body_full);
    let entry = make_entry(&server.uri(), &server.uri(), &sha, 10);

    download_model_file(&client(), &entry.sources, &entry.files[0], &model_dir, None)
        .await
        .unwrap();

    let range = captured_range.lock().unwrap().clone();
    assert_eq!(
        range.as_deref(),
        Some("bytes=5-"),
        "未发送正确 Range 头：{range:?}"
    );

    // 最终文件内容应为完整 10 字节
    let final_content = tokio::fs::read(model_dir.join("model.bin")).await.unwrap();
    assert_eq!(final_content, b"helloworld");
}

// ── 测试 2：首次下载无 Range 头 ────────────────────────────────────────────────

/// 无 .part 文件时，不应发送 Range 头（全量下载）。
#[tokio::test]
async fn no_range_header_on_fresh_download() {
    let server = MockServer::start().await;
    let has_range = Arc::new(Mutex::new(false));
    let has_range2 = has_range.clone();

    let body = b"hello";
    let sha = sha256_bytes(body);

    Mock::given(method("GET"))
        .and(path("/model.bin"))
        .respond_with(move |req: &Request| {
            if req.headers.get("range").is_some() {
                *has_range2.lock().unwrap() = true;
            }
            ResponseTemplate::new(200).set_body_bytes(body.to_vec())
        })
        .mount(&server)
        .await;

    let data_dir = tempdir().unwrap();
    let model_dir = data_dir.path().join("models").join("test-model");
    let entry = make_entry(&server.uri(), &server.uri(), &sha, body.len() as u64);

    download_model_file(&client(), &entry.sources, &entry.files[0], &model_dir, None)
        .await
        .unwrap();

    assert!(!*has_range.lock().unwrap(), "不应发送 Range 头");
}

#[tokio::test]
async fn download_request_sends_typex_user_agent() {
    let server = MockServer::start().await;
    let captured_user_agent = Arc::new(Mutex::new(None::<String>));
    let captured_user_agent2 = captured_user_agent.clone();

    let body = b"hello";
    let sha = sha256_bytes(body);

    Mock::given(method("GET"))
        .and(path("/model.bin"))
        .respond_with(move |req: &Request| {
            let user_agent = req
                .headers
                .get("user-agent")
                .map(|v| v.to_str().unwrap_or("").to_string());
            *captured_user_agent2.lock().unwrap() = user_agent;
            ResponseTemplate::new(200).set_body_bytes(body.to_vec())
        })
        .mount(&server)
        .await;

    let data_dir = tempdir().unwrap();
    let model_dir = data_dir.path().join("models").join("test-model");
    let entry = make_entry(&server.uri(), &server.uri(), &sha, body.len() as u64);

    download_model_file(&client(), &entry.sources, &entry.files[0], &model_dir, None)
        .await
        .unwrap();

    let user_agent = captured_user_agent.lock().unwrap().clone();
    assert!(
        user_agent
            .as_deref()
            .is_some_and(|v| v.starts_with("Typex/")),
        "未发送 Typex User-Agent：{user_agent:?}"
    );
}

// ── 测试 3：断点续传——.part 文件保留，重启后追加 ────────────────────────────────

/// 模拟首次下载中断（只收到前半段），再次调用 download_model_file 时：
/// 1. 发送 Range 续传请求。
/// 2. 最终文件内容完整。
#[tokio::test]
async fn resume_appends_to_part_file() {
    let server = MockServer::start().await;
    let call_count = Arc::new(Mutex::new(0u32));
    let call_count2 = call_count.clone();

    let full_body: Vec<u8> = (0u8..20).collect();
    let sha = sha256_bytes(&full_body);
    let full_body_clone = full_body.clone();

    Mock::given(method("GET"))
        .and(path("/model.bin"))
        .respond_with(move |req: &Request| {
            let mut count = call_count2.lock().unwrap();
            *count += 1;
            if let Some(range_val) = req.headers.get("range") {
                // 续传请求：解析偏移
                let range_str = range_val.to_str().unwrap_or("");
                // "bytes=N-"
                let offset: usize = range_str
                    .strip_prefix("bytes=")
                    .and_then(|s| s.strip_suffix('-'))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let tail = full_body_clone[offset..].to_vec();
                ResponseTemplate::new(206)
                    .insert_header(
                        "Content-Range",
                        format!(
                            "bytes {}-{}/{}",
                            offset,
                            full_body_clone.len() - 1,
                            full_body_clone.len()
                        ),
                    )
                    .set_body_bytes(tail)
            } else {
                // 首次全量 → 只返回前 10 字节（模拟中断）
                ResponseTemplate::new(200).set_body_bytes(full_body_clone[..10].to_vec())
            }
        })
        .mount(&server)
        .await;

    let data_dir = tempdir().unwrap();
    let model_dir = data_dir.path().join("models").join("test-model");
    tokio::fs::create_dir_all(&model_dir).await.unwrap();

    let entry = make_entry(&server.uri(), &server.uri(), &sha, full_body.len() as u64);

    // 第一次下载（只收到前 10 字节，SHA 校验因文件不完整而跳过，但文件已写入 .part）
    // 使用空 sha256 让第一次不校验（测试聚焦续传逻辑）
    let _entry_no_sha = make_entry(&server.uri(), &server.uri(), "", full_body.len() as u64);
    // 手动写入 .part 文件（模拟"第一次下载中断"留下的残片）
    let part_path = model_dir.join("model.bin.part");
    tokio::fs::write(&part_path, &full_body[..10])
        .await
        .unwrap();

    // 第二次：携带真实 sha256，触发 Range 续传
    download_model_file(&client(), &entry.sources, &entry.files[0], &model_dir, None)
        .await
        .unwrap();

    let final_content = tokio::fs::read(model_dir.join("model.bin")).await.unwrap();
    assert_eq!(final_content, full_body, "续传后内容不完整");
}

// ── 测试 4：校验失败换源重试 ──────────────────────────────────────────────────

/// HuggingFace 返回数据 SHA-256 不匹配 → 删除 .part → 切换 ModelScope → 校验通过。
#[tokio::test]
async fn checksum_failure_switches_to_second_source() {
    let hf_server = MockServer::start().await;
    let ms_server = MockServer::start().await;

    let good_body = b"correct-content";
    let bad_body = b"corrupted!xxxxx";
    let expected_sha = sha256_bytes(good_body);

    // HF 返回损坏数据（SHA 不匹配）
    Mock::given(method("GET"))
        .and(path("/model.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bad_body.to_vec()))
        .mount(&hf_server)
        .await;

    // ModelScope 返回正确数据
    Mock::given(method("GET"))
        .and(path("/model.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(good_body.to_vec()))
        .mount(&ms_server)
        .await;

    let data_dir = tempdir().unwrap();
    let model_dir = data_dir.path().join("models").join("test-model");
    let entry = make_entry(
        &hf_server.uri(),
        &ms_server.uri(),
        &expected_sha,
        good_body.len() as u64,
    );

    download_model_file(&client(), &entry.sources, &entry.files[0], &model_dir, None)
        .await
        .expect("换源后应成功");

    let content = tokio::fs::read(model_dir.join("model.bin")).await.unwrap();
    assert_eq!(content, good_body.as_slice());

    // .part 文件应已清理
    assert!(!model_dir.join("model.bin.part").exists(), ".part 未清理");
}

// ── 测试 5：两源均失败 → AllSourcesFailed ─────────────────────────────────────

#[tokio::test]
async fn all_sources_fail_returns_error() {
    let hf_server = MockServer::start().await;
    let ms_server = MockServer::start().await;

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&hf_server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&ms_server)
        .await;

    let data_dir = tempdir().unwrap();
    let model_dir = data_dir.path().join("models").join("test-model");
    let entry = make_entry(&hf_server.uri(), &ms_server.uri(), "", 100);

    let err = download_model_file(&client(), &entry.sources, &entry.files[0], &model_dir, None)
        .await
        .unwrap_err();

    assert!(
        matches!(err, DownloadError::AllSourcesFailed { .. }),
        "期望 AllSourcesFailed，得到：{err:?}"
    );
}

// ── 测试 6：进度回调严格单调递增 ─────────────────────────────────────────────

/// 进度回调接收到的 `downloaded` 字段必须严格单调递增（不倒退）。
#[tokio::test]
async fn progress_callback_is_monotonically_increasing() {
    let server = MockServer::start().await;
    let body: Vec<u8> = (0u8..=255).cycle().take(1024).collect();
    let sha = sha256_bytes(&body);

    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    let data_dir = tempdir().unwrap();
    let model_dir = data_dir.path().join("models").join("test-model");
    let entry = make_entry(&server.uri(), &server.uri(), &sha, body.len() as u64);

    let progress_log: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
    let progress_log2 = progress_log.clone();
    let cb: ProgressFn = Box::new(move |p: Progress| {
        progress_log2.lock().unwrap().push(p.downloaded);
    });

    download_model_file(
        &client(),
        &entry.sources,
        &entry.files[0],
        &model_dir,
        Some(cb),
    )
    .await
    .unwrap();

    let log = progress_log.lock().unwrap().clone();
    assert!(!log.is_empty(), "进度回调未触发");
    for window in log.windows(2) {
        assert!(
            window[1] >= window[0],
            "进度倒退：{} → {}",
            window[0],
            window[1]
        );
    }
    // 最终值应等于文件字节数
    assert_eq!(*log.last().unwrap(), body.len() as u64);
}

// ── 测试 7：list_downloaded / delete_model ──────────────────────────────────

#[tokio::test]
async fn list_and_delete_model() {
    let data_dir = tempdir().unwrap();
    let catalog = typex_lib::local::manifest::catalog();

    // 初始无已下载模型
    let downloaded = list_downloaded(data_dir.path(), &catalog);
    assert!(downloaded.is_empty());

    // 手动"安装"第一个模型（写入所有文件）
    let entry = &catalog[0];
    let model_dir = data_dir.path().join("models").join(&entry.id);
    tokio::fs::create_dir_all(&model_dir).await.unwrap();
    for file in &entry.files {
        tokio::fs::write(model_dir.join(&file.name), b"stub")
            .await
            .unwrap();
    }

    let downloaded = list_downloaded(data_dir.path(), &catalog);
    assert_eq!(downloaded, vec![entry.id.clone()]);

    // 删除
    delete_model(data_dir.path(), entry).await.unwrap();
    let downloaded = list_downloaded(data_dir.path(), &catalog);
    assert!(downloaded.is_empty());
}

// ── 测试 8：delete_model 不存在时静默成功 ─────────────────────────────────────

#[tokio::test]
async fn delete_nonexistent_model_ok() {
    let data_dir = tempdir().unwrap();
    let catalog = typex_lib::local::manifest::catalog();
    let result = delete_model(data_dir.path(), &catalog[0]).await;
    assert!(result.is_ok());
}

// ── 测试 9：已完整下载的文件跳过重下载 ────────────────────────────────────────

#[tokio::test]
async fn skip_already_downloaded_file() {
    let server = MockServer::start().await;
    // server 未挂 mock，若发出请求会 500（wiremock 默认无匹配 → 404）
    // 目标：download_model_file 在文件已存在时完全不发请求

    let data_dir = tempdir().unwrap();
    let model_dir = data_dir.path().join("models").join("test-model");
    tokio::fs::create_dir_all(&model_dir).await.unwrap();
    tokio::fs::write(model_dir.join("model.bin"), b"existing")
        .await
        .unwrap();

    let entry = make_entry(&server.uri(), &server.uri(), "", 8);
    // 文件已存在 → 应直接返回 Ok，不发网络请求
    download_model_file(&client(), &entry.sources, &entry.files[0], &model_dir, None)
        .await
        .expect("已存在文件应跳过");
}
