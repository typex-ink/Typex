//! volcengine STT（03 §2.2）：火山/豆包「大模型录音文件识别——极速版 flash」。
//!
//! 完全自有协议：JSON body + base64 音频，双凭据 header（AppKey + AccessToken），
//! 成功判定看响应 header `X-Api-Status-Code: 20000000`，文本在 `result.text`。

use super::{AudioInput, SttCapabilities, SttOptions, SttProvider, Transcript};
use crate::providers::{ProviderError, http};
use base64::Engine;

/// 官方端点（03 §2.2）；base_url 留空时使用。
pub const DEFAULT_URL: &str =
    "https://openspeech.bytedance.com/api/v3/auc/bigmodel/recognize/flash";
/// 极速版资源 ID（可经 profile options `resource_id` 覆盖）。
pub const DEFAULT_RESOURCE_ID: &str = "volc.bigasr.auc_turbo";
/// 成功状态码（响应 header `X-Api-Status-Code`）。
const STATUS_OK: &str = "20000000";

pub struct VolcengineStt {
    client: reqwest::Client,
    url: String,
    app_key: String,
    access_key: String,
    resource_id: String,
}

impl VolcengineStt {
    pub fn new(
        client: reqwest::Client,
        base_url: impl Into<String>,
        app_key: impl Into<String>,
        access_key: impl Into<String>,
    ) -> Self {
        let base_url = base_url.into();
        Self {
            client,
            url: if base_url.is_empty() {
                DEFAULT_URL.to_string()
            } else {
                base_url
            },
            app_key: app_key.into(),
            access_key: access_key.into(),
            resource_id: DEFAULT_RESOURCE_ID.to_string(),
        }
    }

    pub fn with_resource_id(mut self, resource_id: impl Into<String>) -> Self {
        self.resource_id = resource_id.into();
        self
    }

    fn build_body(&self, audio: &AudioInput) -> serde_json::Value {
        serde_json::json!({
            "user": { "uid": "typex" },
            "audio": {
                "format": "wav",
                "data": base64::engine::general_purpose::STANDARD.encode(&audio.wav_16k_mono),
            },
            "request": {
                "model_name": "bigmodel",
                "enable_punc": true,
                "enable_itn": true,
            },
        })
    }
}

#[derive(serde::Deserialize)]
struct FlashResponse {
    #[serde(default)]
    result: Option<FlashResult>,
}

#[derive(serde::Deserialize)]
struct FlashResult {
    #[serde(default)]
    text: String,
}

/// 火山业务状态码 → 统一错误分类（03 §1）。
/// 45xxxxxx = 客户端参数/鉴权类；55xxxxxx = 服务端。鉴权失败为 45000001/403 系。
fn classify_status(code: &str, body: String) -> ProviderError {
    match code {
        // 官方文档：45000001 请求参数无效；45000002 空音频；45000151 音频格式不正确
        "45000001" | "45000002" | "45000151" => ProviderError::InvalidRequest(body),
        // 鉴权/资源未开通
        c if c.starts_with("403") || c == "45000030" => ProviderError::Auth(body),
        // 限流/并发超限
        "45000429" | "42901003" => ProviderError::RateLimited(body),
        c if c.starts_with("55") => ProviderError::Server {
            status: 500,
            body: format!("volc status {c}: {body}"),
        },
        c => ProviderError::InvalidRequest(format!("volc status {c}: {body}")),
    }
}

#[async_trait::async_trait]
impl SttProvider for VolcengineStt {
    async fn transcribe(
        &self,
        audio: AudioInput,
        _opts: SttOptions,
    ) -> Result<Transcript, ProviderError> {
        let body = self.build_body(&audio);
        http::with_retry(|| async {
            let resp = self
                .client
                .post(&self.url)
                .header("X-Api-App-Key", &self.app_key)
                .header("X-Api-Access-Key", &self.access_key)
                .header("X-Api-Resource-Id", &self.resource_id)
                .header("X-Api-Request-Id", uuid::Uuid::new_v4().to_string())
                .json(&body)
                .send()
                .await
                .map_err(ProviderError::from_reqwest)?;

            let http_status = resp.status().as_u16();
            let api_status = resp
                .headers()
                .get("X-Api-Status-Code")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let text = resp.text().await.map_err(ProviderError::from_reqwest)?;

            // 协议以 X-Api-Status-Code 为准；缺失时退回 HTTP 状态码判定
            match api_status.as_deref() {
                Some(STATUS_OK) => {}
                Some(code) => return Err(classify_status(code, text)),
                None if http_status >= 400 => {
                    return Err(ProviderError::from_status(http_status, text));
                }
                None => {
                    return Err(ProviderError::InvalidRequest(format!(
                        "响应缺少 X-Api-Status-Code: {text}"
                    )));
                }
            }

            let parsed: FlashResponse = serde_json::from_str(&text).map_err(|e| {
                ProviderError::InvalidRequest(format!("响应解析失败: {e}; body: {text}"))
            })?;
            Ok(Transcript {
                text: parsed.result.map(|r| r.text).unwrap_or_default(),
                detected_language: None,
            })
        })
        .await
    }

    fn capabilities(&self) -> SttCapabilities {
        SttCapabilities {
            // 极速版官方限制音频 ≤ 100 MB / 2 小时；取字节上限做切片阈值
            max_bytes: Some(100 * 1024 * 1024),
            supports_prompt: false, // 热词经 corpus 参数（F-10 预留）
            supports_language: false,
        }
    }
}
