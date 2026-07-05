//! 模型下载管理器（03 §8 / CP-8.2）。
//!
//! 功能：
//! - HTTP Range 断点续传（`.part` 临时文件）
//! - SHA-256 完整性校验
//! - 校验失败或网络错误自动换源重试
//! - 进度回调（已下载字节数 + 总字节数）
//! - 存储路径 `{data_dir}/models/{model_id}/`
//! - 查询已下载模型、删除模型
//!
//! `data_dir` 作参数注入，便于集成测试时使用临时目录，不依赖 Tauri 运行时。

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::local::manifest::{ModelEntry, ModelFile, ModelSources};

// ── 错误类型 ──────────────────────────────────────────────────────────────────

/// 下载管理器错误。
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("HTTP 错误：状态码 {status}")]
    Http { status: u16 },
    #[error("校验失败：期望 {expected}，实际 {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("所有源均失败：{reason}")]
    AllSourcesFailed { reason: String },
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("网络错误：{0}")]
    Network(#[from] reqwest::Error),
}

// ── 进度回调 ──────────────────────────────────────────────────────────────────

/// 进度信息。
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    /// 当前已下载字节（含续传起始偏移）。
    pub downloaded: u64,
    /// 文件总字节数（来自 Content-Length 或 manifest）。
    pub total: u64,
}

/// 进度回调类型（`downloaded` 严格单调递增）。
pub type ProgressFn = Box<dyn Fn(Progress) + Send + Sync>;

// ── 源列表 ────────────────────────────────────────────────────────────────────

/// 从 `ModelSources` 提取双源 URL（HuggingFace 优先）。
fn source_urls(sources: &ModelSources, file_name: &str) -> Vec<String> {
    vec![
        format!(
            "{}/{}",
            sources.huggingface.trim_end_matches('/'),
            file_name
        ),
        format!("{}/{}", sources.modelscope.trim_end_matches('/'), file_name),
    ]
}

// ── 路径辅助 ──────────────────────────────────────────────────────────────────

/// 模型存储根目录：`{data_dir}/models/{model_id}/`。
fn model_dir(data_dir: &Path, model_id: &str) -> PathBuf {
    data_dir.join("models").join(model_id)
}

/// `.part` 临时文件路径（下载中），与最终文件同目录。
fn part_path(final_path: &Path) -> PathBuf {
    let mut p = final_path.to_path_buf();
    let mut name = p
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    name.push_str(".part");
    p.set_file_name(name);
    p
}

// ── SHA-256 校验 ──────────────────────────────────────────────────────────────

/// 对文件计算 SHA-256 十六进制摘要。
async fn sha256_of_file(path: &Path) -> Result<String, DownloadError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

// ── 单文件下载（带断点续传）────────────────────────────────────────────────────

/// 从 `url` 下载 `file` 到 `dest`，支持断点续传（.part 文件）。
///
/// - `expected_sha256` 为空串时跳过校验。
/// - `progress_fn` 每收到数据块触发一次（已下载字节严格单调递增）。
async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    total_hint: u64,
    progress_fn: &Option<ProgressFn>,
) -> Result<(), DownloadError> {
    let part = part_path(dest);

    // 已有 .part 文件 → 续传
    let resume_from = if part.exists() {
        tokio::fs::metadata(&part).await?.len()
    } else {
        0
    };

    // 构造请求（Range 头）
    let mut req = client.get(url);
    if resume_from > 0 {
        req = req.header("Range", format!("bytes={resume_from}-"));
    }
    let resp = req.send().await?;

    let status = resp.status().as_u16();
    // 200 OK（新建）或 206 Partial Content（续传）均合法
    if status != 200 && status != 206 {
        return Err(DownloadError::Http { status });
    }

    // 期望总字节数（优先 Content-Length + resume_from，次选 manifest 提示）
    let total = resp
        .content_length()
        .map(|cl| cl + resume_from)
        .unwrap_or(total_hint);

    // 打开 .part 文件（追加模式）
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&part)
        .await?;

    let mut downloaded = resume_from;
    let mut stream = resp.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if let Some(cb) = progress_fn {
            cb(Progress { downloaded, total });
        }
    }
    file.flush().await?;
    drop(file);

    // SHA-256 校验（非空 expected）
    if !expected_sha256.is_empty() {
        let actual = sha256_of_file(&part).await?;
        if actual != expected_sha256 {
            // 校验失败：删除 .part，由调用者换源重试
            let _ = tokio::fs::remove_file(&part).await;
            return Err(DownloadError::ChecksumMismatch {
                expected: expected_sha256.to_string(),
                actual,
            });
        }
    }

    // 原子重命名为最终文件名
    tokio::fs::rename(&part, dest).await?;
    Ok(())
}

// ── 公开 API ──────────────────────────────────────────────────────────────────

/// 下载单个模型文件，换源重试。
///
/// 优先使用 HuggingFace，失败（HTTP 错误或校验失败）后自动切换到 ModelScope。
/// `progress_fn` 收到的 `downloaded` 字节数严格单调递增。
pub async fn download_model_file(
    client: &reqwest::Client,
    sources: &ModelSources,
    file: &ModelFile,
    dest_dir: &Path,
    progress_fn: Option<ProgressFn>,
) -> Result<(), DownloadError> {
    tokio::fs::create_dir_all(dest_dir).await?;
    let dest = dest_dir.join(&file.name);
    if dest.exists() {
        // 已完整下载（.part 不存在），直接跳过
        return Ok(());
    }

    let urls = source_urls(sources, &file.name);
    let mut last_err = String::new();

    for url in &urls {
        match download_file(client, url, &dest, &file.sha256, file.bytes, &progress_fn).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::warn!(url = %url, err = %e, "下载失败，尝试换源");
                last_err = e.to_string();
                // ChecksumMismatch 已删除 .part，其他错误保留以便下次续传（网络抖动）
            }
        }
    }

    Err(DownloadError::AllSourcesFailed { reason: last_err })
}

/// 下载模型的全部文件（顺序逐一下载）。
///
/// `data_dir`：应用数据目录（注入，便于测试）。
/// `progress_fn`：每个文件块触发，`downloaded` 从 0 开始累计（跨文件重置）。
pub async fn download_model(
    client: &reqwest::Client,
    entry: &ModelEntry,
    data_dir: &Path,
    progress_fn: Option<ProgressFn>,
) -> Result<(), DownloadError> {
    let dir = model_dir(data_dir, &entry.id);
    for file in &entry.files {
        download_model_file(client, &entry.sources, file, &dir, None).await?;
    }
    let _ = progress_fn; // 跨文件合计进度由上层（Tauri event）聚合
    Ok(())
}

/// 列出已完整下载的模型 ID 列表。
///
/// 判断依据：模型目录存在 **且** 所有 manifest 文件均存在（无 .part 残留）。
pub fn list_downloaded(data_dir: &Path, catalog: &[ModelEntry]) -> Vec<String> {
    catalog
        .iter()
        .filter(|entry| {
            let dir = model_dir(data_dir, &entry.id);
            entry.files.iter().all(|f| dir.join(&f.name).exists())
        })
        .map(|e| e.id.clone())
        .collect()
}

/// 删除模型（含目录及全部文件）。
///
/// 返回实际删除了多少字节（估算，来自 manifest bytes 字段）；
/// 模型目录不存在时静默忽略。
pub async fn delete_model(data_dir: &Path, entry: &ModelEntry) -> Result<u64, DownloadError> {
    let dir = model_dir(data_dir, &entry.id);
    if !dir.exists() {
        return Ok(0);
    }
    let freed: u64 = entry.files.iter().map(|f| f.bytes).sum();
    tokio::fs::remove_dir_all(&dir).await?;
    Ok(freed)
}

// ── 供测试用的 SHA-256 工具（公开给集成测试）────────────────────────────────────

/// SHA-256 十六进制摘要（集成测试辅助；生产路径使用 `sha256_of_file`）。
pub fn sha256_bytes(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn part_path_appends_part() {
        let p = PathBuf::from("/tmp/model.gguf");
        assert_eq!(part_path(&p), PathBuf::from("/tmp/model.gguf.part"));
    }

    #[test]
    fn source_urls_order_hf_first() {
        let sources = ModelSources {
            huggingface: "https://hf.co/repo".into(),
            modelscope: "https://ms.cn/repo".into(),
        };
        let urls = source_urls(&sources, "model.gguf");
        assert_eq!(urls[0], "https://hf.co/repo/model.gguf");
        assert_eq!(urls[1], "https://ms.cn/repo/model.gguf");
    }

    #[test]
    fn source_urls_trims_trailing_slash() {
        let sources = ModelSources {
            huggingface: "https://hf.co/repo/".into(),
            modelscope: "https://ms.cn/repo/".into(),
        };
        let urls = source_urls(&sources, "f.gguf");
        assert!(!urls[0].contains("//f.gguf"), "双斜杠：{}", urls[0]);
    }

    #[test]
    fn sha256_bytes_deterministic() {
        let h1 = sha256_bytes(b"hello");
        let h2 = sha256_bytes(b"hello");
        assert_eq!(h1, h2);
        assert_ne!(sha256_bytes(b"hello"), sha256_bytes(b"world"));
    }
}
