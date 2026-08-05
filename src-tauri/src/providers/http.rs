//! 共享 reqwest 客户端工厂 + 统一重试（06 §5.4：重试策略集中在此，orchestrator 不重复实现）。

use super::ProviderError;
use crate::settings::schema::ProxyMode;
use std::time::Duration;

/// 按代理设置构造客户端（06 §9：默认跟随系统代理）。
///
/// STT/LLM 的完整调用时限由统一 Provider 包装层执行，HTTP adapter 不另设总时限。
pub fn build_client(proxy_mode: ProxyMode, proxy_url: &str) -> reqwest::Client {
    let mut b = reqwest::Client::builder();
    match proxy_mode {
        ProxyMode::System => {} // reqwest 默认读系统代理环境变量
        ProxyMode::Direct => b = b.no_proxy(),
        ProxyMode::Manual => {
            if let Ok(p) = reqwest::Proxy::all(proxy_url) {
                b = b.proxy(p);
            }
        }
    }
    b.build().unwrap_or_default()
}

/// 指数退避重试 ×2（03 §1：仅 retryable 错误；Auth/InvalidRequest 立即失败）。
pub async fn with_retry<T, F, Fut>(mut op: F) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, ProviderError>>,
{
    let mut delay = Duration::from_millis(500);
    let mut last_err = None;
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(delay).await;
            delay *= 2;
        }
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) if e.retryable() && attempt < 2 => {
                tracing::info!(
                    attempt = attempt + 1,
                    error_code = ?e.error_code(),
                    http_status = ?e.http_status(),
                    response_body_len = e.response_body_len(),
                    "provider 调用失败将重试"
                );
                last_err = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.expect("retry loop"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct TestWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for TestWriter {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn retries_retryable_twice_then_gives_up() {
        let calls = AtomicU32::new(0);
        let r: Result<(), _> = with_retry(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(ProviderError::Network("x".into())) }
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3); // 1 + 2 重试
    }

    #[tokio::test]
    async fn auth_error_not_retried() {
        let calls = AtomicU32::new(0);
        let r: Result<(), _> = with_retry(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(ProviderError::Auth("401".into())) }
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn success_after_transient_failure() {
        let calls = AtomicU32::new(0);
        let r = with_retry(|| {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    Err(ProviderError::Server {
                        status: 503,
                        message: String::new(),
                    })
                } else {
                    Ok(42)
                }
            }
        })
        .await;
        assert_eq!(r.unwrap(), 42);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn retry_log_excludes_upstream_response_content() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let writer = TestWriter(Arc::clone(&output));
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let response = r#"{"error":{"message":"private-summary","sentinel":"private-body"}}"#;

        let result: Result<(), _> = with_retry(|| {
            let response = response.to_string();
            async move { Err(ProviderError::from_status(503, response)) }
        })
        .await;

        assert!(result.is_err());
        let logs = String::from_utf8(output.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("ServerError"));
        assert!(logs.contains("http_status=Some(503)"));
        assert!(logs.contains("response_body_len="));
        assert!(!logs.contains("private-summary"));
        assert!(!logs.contains("private-body"));
    }
}
