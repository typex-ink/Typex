//! ProviderError 分类（03 §1-4：统一错误分类，重试策略据此决定）。

use crate::error::{ErrorCode, TypexError};
use crate::types::ProfileTestError;

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
    #[error("服务端错误 ({status}): {message}")]
    Server { status: u16, message: String },
    #[error("网络错误: {0}")]
    Network(String),
    #[error("上游请求失败（{code:?}, HTTP {status:?}）")]
    Upstream {
        code: ErrorCode,
        status: Option<u16>,
        message: String,
        body: String,
    },
}

impl ProviderError {
    /// 是否可自动重试（03 §1：RateLimited/ServerError/Network 重试，Auth 不重试）。
    pub fn retryable(&self) -> bool {
        match self {
            ProviderError::RateLimited(_)
            | ProviderError::Server { .. }
            | ProviderError::Network(_)
            | ProviderError::Timeout => true,
            ProviderError::Upstream { code, .. } => matches!(
                code,
                ErrorCode::RateLimited
                    | ErrorCode::ServerError
                    | ErrorCode::NetworkError
                    | ErrorCode::Timeout
            ),
            ProviderError::Auth(_) | ProviderError::InvalidRequest(_) => false,
        }
    }

    /// 由 HTTP 状态码分类。
    pub fn from_status(status: u16, body: String) -> Self {
        let code = match status {
            401 | 403 => ErrorCode::AuthError,
            429 => ErrorCode::RateLimited,
            400 | 404 | 413 | 422 => ErrorCode::InvalidRequest,
            s if s >= 500 => ErrorCode::ServerError,
            _ => ErrorCode::InvalidRequest,
        };
        Self::from_upstream_response(code, Some(status), format!("HTTP {status}"), body)
    }

    /// 流式协议在 HTTP 200 内返回的错误事件。
    pub(crate) fn from_stream_error(body: String) -> Self {
        Self::from_upstream_response(ErrorCode::ServerError, None, "响应失败".to_string(), body)
    }

    /// HTTP 成功但响应不符合 adapter 契约；摘要与原始 body 分开保存。
    pub(crate) fn invalid_response(message: impl Into<String>, body: String) -> Self {
        Self::Upstream {
            code: ErrorCode::InvalidRequest,
            status: None,
            message: message.into(),
            body,
        }
    }

    /// 厂商业务状态错误；优先使用响应内的 message，缺失时使用 adapter 摘要。
    pub(crate) fn from_upstream_response(
        code: ErrorCode,
        status: Option<u16>,
        fallback_message: String,
        body: String,
    ) -> Self {
        let message = upstream_error_message(&body).unwrap_or(fallback_message);
        Self::Upstream {
            code,
            status,
            message,
            body,
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

    pub(crate) fn error_code(&self) -> ErrorCode {
        match self {
            ProviderError::Auth(_) => ErrorCode::AuthError,
            ProviderError::RateLimited(_) => ErrorCode::RateLimited,
            ProviderError::Timeout => ErrorCode::Timeout,
            ProviderError::InvalidRequest(_) => ErrorCode::InvalidRequest,
            ProviderError::Server { .. } => ErrorCode::ServerError,
            ProviderError::Network(_) => ErrorCode::NetworkError,
            ProviderError::Upstream { code, .. } => *code,
        }
    }

    pub(crate) fn http_status(&self) -> Option<u16> {
        match self {
            ProviderError::Server { status, .. } => Some(*status),
            ProviderError::Upstream { status, .. } => *status,
            _ => None,
        }
    }

    pub(crate) fn response_body_len(&self) -> usize {
        match self {
            ProviderError::Upstream { body, .. } => body.len(),
            _ => 0,
        }
    }

    fn details(&self) -> Option<String> {
        let ProviderError::Upstream { body, .. } = self else {
            return None;
        };
        (!body.trim().is_empty()).then(|| body.clone())
    }
}

impl From<ProviderError> for TypexError {
    fn from(e: ProviderError) -> Self {
        let code = e.error_code();
        TypexError::new(code, e.user_message())
    }
}

impl From<ProviderError> for ProfileTestError {
    fn from(error: ProviderError) -> Self {
        Self {
            code: error.error_code(),
            message: error.user_message(),
            details: error.details(),
        }
    }
}

impl ProviderError {
    fn user_message(&self) -> String {
        match self {
            ProviderError::Auth(message)
            | ProviderError::RateLimited(message)
            | ProviderError::InvalidRequest(message)
            | ProviderError::Server { message, .. } => non_empty_message(message, self),
            ProviderError::Network(message) => {
                if message.trim().is_empty() {
                    self.to_string()
                } else {
                    format!("网络错误: {}", message.trim())
                }
            }
            ProviderError::Timeout => self.to_string(),
            ProviderError::Upstream { message, .. } => message.clone(),
        }
    }
}

fn non_empty_message(message: &str, error: &ProviderError) -> String {
    let message = message.trim();
    if message.is_empty() {
        error.to_string()
    } else {
        message.to_string()
    }
}

fn upstream_error_message(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
        && let Some(message) = json_error_message(&value)
    {
        return Some(message);
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
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(candidate)
            && let Some(message) = json_error_message(&value)
        {
            return Some(message);
        }
    }
    concise_text(trimmed)
}

fn json_error_message(value: &serde_json::Value) -> Option<String> {
    value
        .pointer("/response/error/message")
        .and_then(|x| x.as_str())
        .or_else(|| value.pointer("/error/message").and_then(|x| x.as_str()))
        .or_else(|| value.pointer("/message").and_then(|x| x.as_str()))
        .or_else(|| value.pointer("/error").and_then(|x| x.as_str()))
        .and_then(concise_text)
}

fn concise_text(value: &str) -> Option<String> {
    const LIMIT: usize = 240;
    let line = value.lines().map(str::trim).find(|line| !line.is_empty())?;
    let mut chars = line.chars();
    let summary: String = chars.by_ref().take(LIMIT).collect();
    Some(if chars.next().is_some() {
        format!("{summary}…")
    } else {
        summary
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_code_classification_table() {
        let unauthorized = ProviderError::from_status(401, String::new());
        assert_eq!(unauthorized.error_code(), ErrorCode::AuthError);
        assert_eq!(unauthorized.http_status(), Some(401));
        let forbidden = ProviderError::from_status(403, "forbidden".into());
        assert_eq!(forbidden.error_code(), ErrorCode::AuthError);
        assert!(!forbidden.retryable());
        assert_eq!(
            ProviderError::from_status(429, String::new()).error_code(),
            ErrorCode::RateLimited
        );
        assert_eq!(
            ProviderError::from_status(500, String::new()).error_code(),
            ErrorCode::ServerError
        );
        assert_eq!(
            ProviderError::from_status(503, String::new()).error_code(),
            ErrorCode::ServerError
        );
        assert_eq!(
            ProviderError::from_status(400, String::new()).error_code(),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn retry_policy() {
        assert!(!ProviderError::Auth(String::new()).retryable());
        assert!(!ProviderError::InvalidRequest(String::new()).retryable());
        assert!(ProviderError::RateLimited(String::new()).retryable());
        assert!(
            ProviderError::Server {
                status: 500,
                message: String::new()
            }
            .retryable()
        );
        assert!(ProviderError::Network(String::new()).retryable());
        assert!(ProviderError::Timeout.retryable());
        assert!(ProviderError::from_status(429, String::new()).retryable());
        assert!(ProviderError::from_status(503, String::new()).retryable());
        assert!(!ProviderError::from_status(401, String::new()).retryable());
    }

    #[test]
    fn typex_error_uses_upstream_error_message() {
        let body = r#"{"error":{"message":"client group denied","type":"forbidden_error"}}"#;
        let err: TypexError = ProviderError::from_status(403, body.into()).into();
        assert_eq!(err.code, ErrorCode::AuthError);
        assert_eq!(err.message, "client group denied");
    }

    #[test]
    fn profile_test_error_keeps_complete_upstream_details() {
        let body = r#"{"error":{"message":"Upstream request failed","type":"upstream_error","details":{"error":{"message":"model not found"}},"request_id":"req-123"}}"#;
        let err: ProfileTestError = ProviderError::from_status(400, body.into()).into();

        assert_eq!(err.code, ErrorCode::InvalidRequest);
        assert_eq!(err.message, "Upstream request failed");
        assert_eq!(err.details.as_deref(), Some(body));
    }

    #[test]
    fn profile_test_error_keeps_plain_text_as_details() {
        let err: ProfileTestError =
            ProviderError::from_status(404, "route not found".into()).into();

        assert_eq!(err.message, "route not found");
        assert_eq!(err.details.as_deref(), Some("route not found"));
    }

    #[test]
    fn internal_errors_do_not_claim_response_details() {
        let err: ProfileTestError = ProviderError::InvalidRequest("local failure".into()).into();

        assert_eq!(err.message, "local failure");
        assert_eq!(err.details, None);
    }

    #[test]
    fn log_representation_excludes_upstream_summary_and_body() {
        let body = r#"{"error":{"message":"private-summary","sentinel":"private-body"}}"#;
        let error = ProviderError::from_status(503, body.into());
        let rendered = error.to_string();

        assert!(!rendered.contains("private-summary"));
        assert!(!rendered.contains("private-body"));
        assert_eq!(error.error_code(), ErrorCode::ServerError);
        assert_eq!(error.http_status(), Some(503));
        assert_eq!(error.response_body_len(), body.len());
    }

    #[test]
    fn upstream_message_parser_handles_sse_data_line() {
        let body = "event: error\ndata: {\"error\":{\"message\":\"upstream failed\"}}\n\n";
        assert_eq!(
            upstream_error_message(body).as_deref(),
            Some("upstream failed")
        );
    }

    #[test]
    fn upstream_message_parser_handles_responses_failed_payload() {
        let body = r#"{"response":{"error":{"message":"overloaded","type":"server_error"}}}"#;
        assert_eq!(upstream_error_message(body).as_deref(), Some("overloaded"));
    }
}
