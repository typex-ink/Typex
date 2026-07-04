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
        } else if e.is_connect() || e.is_request() {
            ProviderError::Network(e.to_string())
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
        TypexError::new(code, e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_code_classification_table() {
        assert!(matches!(ProviderError::from_status(401, String::new()), ProviderError::Auth(_)));
        assert!(matches!(ProviderError::from_status(403, String::new()), ProviderError::Auth(_)));
        assert!(matches!(ProviderError::from_status(429, String::new()), ProviderError::RateLimited(_)));
        assert!(matches!(ProviderError::from_status(500, String::new()), ProviderError::Server { .. }));
        assert!(matches!(ProviderError::from_status(503, String::new()), ProviderError::Server { .. }));
        assert!(matches!(ProviderError::from_status(400, String::new()), ProviderError::InvalidRequest(_)));
    }

    #[test]
    fn retry_policy() {
        assert!(!ProviderError::Auth(String::new()).retryable());
        assert!(!ProviderError::InvalidRequest(String::new()).retryable());
        assert!(ProviderError::RateLimited(String::new()).retryable());
        assert!(ProviderError::Server { status: 500, body: String::new() }.retryable());
        assert!(ProviderError::Network(String::new()).retryable());
        assert!(ProviderError::Timeout.retryable());
    }
}
