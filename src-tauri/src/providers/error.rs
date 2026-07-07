//! ProviderError 分类（03 §1-4：统一错误分类，重试策略据此决定）。

use crate::error::{ErrorCode, TypexError};

#[derive(Debug, Clone, thiserror::Error)]
pub enum ProviderError {
    #[error("鉴权失败: {0}")]
    Auth(String),
    #[error("限流: {0}")]
    RateLimited(String),
    #[error("超时")]
    Timeout,
    #[error("非法请求: {0}")]
    InvalidRequest(String),
    #[error("服务端错误 ({status}): {body}")]
    Server { status: u16, body: String },
    #[error("网络错误: {0}")]
    Network(String),
}

impl ProviderError {
    /// 是否可自动重试（03 §1：RateLimited/ServerError/Network 重试，Auth 不重试）。
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimited(_)
                | ProviderError::Server { .. }
                | ProviderError::Network(_)
                | ProviderError::Timeout
        )
    }

    /// 由 HTTP 状态码分类。
    pub fn from_status(status: u16, body: String) -> Self {
        match status {
            401 | 403 => ProviderError::Auth(body),
            429 => ProviderError::RateLimited(body),
            400 | 404 | 413 | 422 => ProviderError::InvalidRequest(body),
            s if s >= 500 => ProviderError::Server { status: s, body },
            _ => ProviderError::InvalidRequest(body),
        }
    }

    /// reqwest 错误分类。
    pub fn from_reqwest(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            ProviderError::Timeout
        } else {
            ProviderError::Network(e.to_string())
        }
    }
}

impl From<ProviderError> for TypexError {
    fn from(e: ProviderError) -> Self {
        let code = match &e {
            ProviderError::Auth(_) => ErrorCode::AuthError,
            ProviderError::RateLimited(_) => ErrorCode::RateLimited,
            ProviderError::Timeout => ErrorCode::Timeout,
            ProviderError::InvalidRequest(_) => ErrorCode::InvalidRequest,
            ProviderError::Server { .. } => ErrorCode::ServerError,
            ProviderError::Network(_) => ErrorCode::NetworkError,
        };
        TypexError::new(code, e.user_message())
    }
}

impl ProviderError {
    fn user_message(&self) -> String {
        match self {
            ProviderError::Auth(body)
            | ProviderError::RateLimited(body)
            | ProviderError::InvalidRequest(body) => {
                upstream_error_message(body).unwrap_or_else(|| self.to_string())
            }
            ProviderError::Server { body, .. } => {
                upstream_error_message(body).unwrap_or_else(|| self.to_string())
            }
            ProviderError::Timeout | ProviderError::Network(_) => self.to_string(),
        }
    }
}

fn upstream_error_message(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    for line in trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let candidate = line.strip_prefix("data:").map(str::trim).unwrap_or(line);
        if !candidate.starts_with('{') {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(candidate) {
            if let Some(msg) = v
                .pointer("/error/message")
                .and_then(|x| x.as_str())
                .or_else(|| v.pointer("/message").and_then(|x| x.as_str()))
                .or_else(|| v.pointer("/error").and_then(|x| x.as_str()))
            {
                let msg = msg.trim();
                if !msg.is_empty() {
                    return Some(msg.to_string());
                }
            }
        }
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_code_classification_table() {
        assert!(matches!(
            ProviderError::from_status(401, String::new()),
            ProviderError::Auth(_)
        ));
        let forbidden = ProviderError::from_status(403, "forbidden".into());
        assert!(matches!(forbidden, ProviderError::Auth(_)));
        assert!(!forbidden.retryable());
        assert!(matches!(
            ProviderError::from_status(429, String::new()),
            ProviderError::RateLimited(_)
        ));
        assert!(matches!(
            ProviderError::from_status(500, String::new()),
            ProviderError::Server { .. }
        ));
        assert!(matches!(
            ProviderError::from_status(503, String::new()),
            ProviderError::Server { .. }
        ));
        assert!(matches!(
            ProviderError::from_status(400, String::new()),
            ProviderError::InvalidRequest(_)
        ));
    }

    #[test]
    fn retry_policy() {
        assert!(!ProviderError::Auth(String::new()).retryable());
        assert!(!ProviderError::InvalidRequest(String::new()).retryable());
        assert!(ProviderError::RateLimited(String::new()).retryable());
        assert!(
            ProviderError::Server {
                status: 500,
                body: String::new()
            }
            .retryable()
        );
        assert!(ProviderError::Network(String::new()).retryable());
        assert!(ProviderError::Timeout.retryable());
    }

    #[test]
    fn typex_error_uses_upstream_error_message() {
        let body = r#"{"error":{"message":"client group denied","type":"forbidden_error"}}"#;
        let err: TypexError = ProviderError::from_status(403, body.into()).into();
        assert_eq!(err.code, ErrorCode::AuthError);
        assert_eq!(err.message, "client group denied");
    }

    #[test]
    fn upstream_message_parser_handles_sse_data_line() {
        let body = "event: error\ndata: {\"error\":{\"message\":\"upstream failed\"}}\n\n";
        assert_eq!(
            upstream_error_message(body).as_deref(),
            Some("upstream failed")
        );
    }
}
