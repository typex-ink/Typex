//! DecisionProvider trait（03 §3.5 / ADR-30）：非自回归结构化概率决策。
pub mod jev;

use super::ProviderError;
use std::sync::Arc;
use std::time::Duration;

/// 整理决策请求中的问题键名（TypeSafe AI System One `questions` 字段键）。
pub const POLISH_DECISION_QUESTION_KEY: &str = "needs_polish";

/// 内置默认判定指令：整理前置决策（03 §3.5 逐字）。
pub const POLISH_DECISION_INSTRUCTIONS: &str = "\
判断 state.transcript 这段语音转写原始文本是否需要经过文本整理模型清理后再输出。当存在以下任一情况时应判定为需要整理（是）：
1. 含有口语填充词、犹豫词、口吃或无意义重复（如“嗯”“啊”“呃”“那个”“就是”、um、uh、like 等）；
2. 含有中途改口、自我纠正或重新起头（如“不对”“我是说”“算了”“应该是”、wait no、I mean 等）；
3. 含有口述标点或排版指令（如“句号”“逗号”“换行”“另起一段”“列成清单”等）；
4. 存在明显的同音错字、语义不通、多句长文缺少必要标点断句、需要规范化的复杂口述日期/数字/金额，或疑似把 state.dictionary_terms 中的专有名词误识别成了近音词。
若文本只是简短词句、问候、确认语，或本身已通顺自然、标点合理、无填充词与改口、无明显识别错误，可直接原样输出，则判定为不需要整理（否）。";

/// 解析生效的整理决策判定指令：自定义为空时回退内置值。
pub fn resolve_decision_instructions(custom: &str) -> &str {
    if custom.trim().is_empty() {
        POLISH_DECISION_INSTRUCTIONS
    } else {
        custom
    }
}

/// 传给决策模型的状态对象（03 §3.5）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DecisionState {
    pub transcript: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_app: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dictionary_terms: Vec<String>,
}

/// 单次二元概率决策请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionRequest {
    pub state: DecisionState,
    pub instructions: String,
}

/// 决策响应：校准概率位于 `0.0..=1.0`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecisionResult {
    pub probability: f64,
}

#[async_trait::async_trait]
pub trait DecisionProvider: Send + Sync {
    async fn decide(&self, req: DecisionRequest) -> Result<DecisionResult, ProviderError>;
}

/// 为一个模型服务的所有决策调用统一施加 profile 总时限（覆盖重试全周期）。
pub struct TimedDecisionProvider {
    inner: Arc<dyn DecisionProvider>,
    timeout: Duration,
}

impl TimedDecisionProvider {
    pub fn new(inner: Arc<dyn DecisionProvider>, timeout: Duration) -> Self {
        Self { inner, timeout }
    }
}

#[async_trait::async_trait]
impl DecisionProvider for TimedDecisionProvider {
    async fn decide(&self, req: DecisionRequest) -> Result<DecisionResult, ProviderError> {
        let deadline = tokio::time::Instant::now()
            .checked_add(self.timeout)
            .ok_or_else(|| ProviderError::InvalidRequest("调用超时超出范围".into()))?;
        tokio::time::timeout_at(deadline, self.inner.decide(req))
            .await
            .map_err(|_| ProviderError::Timeout)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_instructions_falls_back_when_blank() {
        assert_eq!(
            resolve_decision_instructions(""),
            POLISH_DECISION_INSTRUCTIONS
        );
        assert_eq!(
            resolve_decision_instructions("   \n  "),
            POLISH_DECISION_INSTRUCTIONS
        );
        assert_eq!(resolve_decision_instructions("自定义规则"), "自定义规则");
    }

    #[test]
    fn state_serialization_omits_optional_empty_fields() {
        let minimal = DecisionState {
            transcript: "好的".into(),
            target_app: None,
            dictionary_terms: Vec::new(),
        };
        assert_eq!(
            serde_json::to_value(&minimal).unwrap(),
            serde_json::json!({ "transcript": "好的" })
        );

        let full = DecisionState {
            transcript: "type x".into(),
            target_app: Some("Slack".into()),
            dictionary_terms: vec!["Typex".into()],
        };
        assert_eq!(
            serde_json::to_value(&full).unwrap(),
            serde_json::json!({
                "transcript": "type x",
                "target_app": "Slack",
                "dictionary_terms": ["Typex"]
            })
        );
    }
}
