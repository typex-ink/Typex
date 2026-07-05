//! tracing 初始化（07 §5.5）。
//!
//! 双重防线：
//! 1. 编码纪律——providers 与 orchestrator 的日志语句只记长度与耗时，不含内容与凭据；
//! 2. redact 写入层（CP-6.11）——所有日志行写盘/上屏前过 `redact()`，
//!    自动打码 Bearer token 与 sk- 形态密钥，兜住手滑。

use std::io::Write;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// 打码一行日志：Bearer/密钥形态字符串 → `[REDACTED]`。
pub fn redact(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    // Bearer <token>（无 regex 依赖；日志不在热路径）
    while let Some(pos) = rest.find("Bearer ") {
        out.push_str(&rest[..pos]);
        out.push_str("Bearer [REDACTED]");
        let after = &rest[pos + 7..];
        let end = after
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);

    // sk- 系密钥（OpenAI/DeepSeek 等惯用前缀；≥12 位密钥体视为密钥）
    let mut redacted = String::with_capacity(out.len());
    let mut segs = out.split("sk-");
    if let Some(first) = segs.next() {
        redacted.push_str(first);
    }
    for seg in segs {
        let keylen = seg
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(seg.len());
        if keylen >= 12 {
            redacted.push_str("sk-[REDACTED]");
            redacted.push_str(&seg[keylen..]);
        } else {
            redacted.push_str("sk-");
            redacted.push_str(seg);
        }
    }
    redacted
}

/// 包一层 Writer：整行缓冲 → redact → 下游。
struct RedactWriter<W: Write> {
    inner: W,
    buf: Vec<u8>,
}

impl<W: Write> Write for RedactWriter<W> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&line);
            self.inner.write_all(redact(&text).as_bytes())?;
        }
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if !self.buf.is_empty() {
            let text = String::from_utf8_lossy(&self.buf).to_string();
            self.inner.write_all(redact(&text).as_bytes())?;
            self.buf.clear();
        }
        self.inner.flush()
    }
}

impl<W: Write> Drop for RedactWriter<W> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// 多 worker 线程共享同一 appender。
struct SharedWriter(
    std::sync::Arc<std::sync::Mutex<tracing_appender::rolling::RollingFileAppender>>,
);

impl Write for SharedWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(data)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

pub fn init(log_dir: Option<std::path::PathBuf>) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(filter);
    match log_dir {
        Some(dir) => {
            let appender = tracing_appender::rolling::daily(dir, "typex.log");
            let shared = std::sync::Arc::new(std::sync::Mutex::new(appender));
            registry
                .with(
                    fmt::layer()
                        .with_writer(move || RedactWriter {
                            inner: SharedWriter(shared.clone()),
                            buf: Vec::new(),
                        })
                        .with_ansi(false),
                )
                .init();
        }
        None => {
            registry
                .with(fmt::layer().with_writer(|| RedactWriter {
                    inner: std::io::stderr(),
                    buf: Vec::new(),
                }))
                .init();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_token_redacted() {
        let line = r#"request failed: Authorization: Bearer sk-abc123def456ghi789 status 401"#;
        let r = redact(line);
        assert!(!r.contains("abc123def456ghi789"), "{r}");
        assert!(r.contains("Bearer [REDACTED]"));
    }

    #[test]
    fn sk_key_redacted_anywhere() {
        let r = redact("connect with key sk-proj-aaaabbbbccccdddd to server");
        assert!(!r.contains("sk-proj-aaaabbbbccccdddd"), "{r}");
        assert!(r.contains("sk-[REDACTED]"));
    }

    #[test]
    fn short_sk_prefix_untouched() {
        // 「sk-8」这类短串不是密钥（比如型号名），不打码
        let r = redact("model sk-8 selected");
        assert!(r.contains("sk-8"));
    }

    #[test]
    fn normal_line_unchanged() {
        let line = "转写完成: 长度 42 字, 耗时 890ms";
        assert_eq!(redact(line), line);
    }
}
