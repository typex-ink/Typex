//! TypeSafe AI System One Jev 决策模型 adapter（03 §3.5）：POST {base_url}/systemone。

use super::{
    DecisionProvider, DecisionRequest, DecisionResult, DecisionState, POLISH_DECISION_QUESTION_KEY,
};
use crate::providers::{ProviderError, http};
use std::collections::HashMap;

pub struct JevDecisionProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    extra_headers: HashMap<String, String>,
}

impl JevDecisionProvider {
    pub fn new(
        client: reqwest::Client,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client,
            base_url: normalize_base_url(&base_url.into()),
            api_key: api_key.into(),
            model: model.into(),
            extra_headers: HashMap::new(),
        }
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.extra_headers = headers;
        self
    }

    fn build_body<'a>(&'a self, req: &'a DecisionRequest) -> JevRequest<'a> {
        JevRequest {
            model: &self.model,
            state: &req.state,
            questions: JevQuestions {
                needs_polish: JevNoulQuestion {
                    kind: "noul",
                    instructions: &req.instructions,
                },
            },
        }
    }
}

fn normalize_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/systemone")
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_string()
}

#[derive(serde::Serialize)]
struct JevRequest<'a> {
    model: &'a str,
    state: &'a DecisionState,
    questions: JevQuestions<'a>,
}

#[derive(serde::Serialize)]
struct JevQuestions<'a> {
    needs_polish: JevNoulQuestion<'a>,
}

#[derive(serde::Serialize)]
struct JevNoulQuestion<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    instructions: &'a str,
}

#[derive(serde::Deserialize)]
struct JevResponse {
    answers: Option<serde_json::Map<String, serde_json::Value>>,
}

fn response_error(reason: &str, body: &str) -> ProviderError {
    ProviderError::invalid_response(format!("Jev {reason}"), body.to_string())
}

fn parse_response(body: &str) -> Result<DecisionResult, ProviderError> {
    let parsed: JevResponse = serde_json::from_str(body)
        .map_err(|error| response_error(&format!("响应 JSON 解析失败: {error}"), body))?;
    let answers = parsed
        .answers
        .ok_or_else(|| response_error("响应 answers 缺失", body))?;
    let answer = answers
        .get(POLISH_DECISION_QUESTION_KEY)
        .ok_or_else(|| response_error("响应缺少 needs_polish 结果", body))?;
    let probability = answer
        .get("noul")
        .ok_or_else(|| response_error("响应 needs_polish.noul 缺失", body))?
        .as_f64()
        .ok_or_else(|| response_error("响应 needs_polish.noul 不是数字", body))?;
    if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
        return Err(response_error("响应 needs_polish.noul 超出 [0, 1]", body));
    }
    Ok(DecisionResult { probability })
}

#[async_trait::async_trait]
impl DecisionProvider for JevDecisionProvider {
    async fn decide(&self, req: DecisionRequest) -> Result<DecisionResult, ProviderError> {
        let url = format!("{}/systemone", self.base_url);
        let body = self.build_body(&req);
        http::with_retry(|| async {
            let mut builder = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&body);
            for (k, v) in &self.extra_headers {
                builder = builder.header(k, v);
            }
            let response = builder.send().await.map_err(ProviderError::from_reqwest)?;
            let status = response.status().as_u16();
            let text = response.text().await.map_err(ProviderError::from_reqwest)?;
            if status >= 400 {
                return Err(ProviderError::from_status(status, text));
            }
            parse_response(&text)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_normalization_deduplicates_systemone_suffix() {
        assert_eq!(
            normalize_base_url("https://api.typesafe.ai/v1"),
            "https://api.typesafe.ai/v1"
        );
        assert_eq!(
            normalize_base_url("https://api.typesafe.ai/v1/"),
            "https://api.typesafe.ai/v1"
        );
        assert_eq!(
            normalize_base_url("https://api.typesafe.ai/v1/systemone"),
            "https://api.typesafe.ai/v1"
        );
        assert_eq!(
            normalize_base_url("https://api.typesafe.ai/v1/systemone/"),
            "https://api.typesafe.ai/v1"
        );
    }

    #[test]
    fn parse_response_validates_noul_bounds_and_preserves_body() {
        let ok = parse_response(
            r#"{"model":"jev-1.14.0","answers":{"needs_polish":{"type":"noul","noul":0.12}}}"#,
        )
        .unwrap();
        assert_eq!(ok.probability, 0.12);

        for invalid in [
            r#"{}"#,
            r#"{"answers":{}}"#,
            r#"{"answers":{"needs_polish":{"type":"noul"}}}"#,
            r#"{"answers":{"needs_polish":{"type":"noul","noul":"high"}}}"#,
            r#"{"answers":{"needs_polish":{"type":"noul","noul":1.2}}}"#,
            r#"{"answers":{"needs_polish":{"type":"noul","noul":-0.1}}}"#,
        ] {
            let err = parse_response(invalid).unwrap_err();
            assert_eq!(err.error_code(), crate::error::ErrorCode::InvalidRequest);
            let test_err = crate::types::ProfileTestError::from(err);
            assert_eq!(test_err.details.as_deref(), Some(invalid));
        }
    }
}
