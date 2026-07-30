//! Xiaomi MiMo ASR（03 §2.3）：JSON POST {base_url}/chat/completions。

use super::{
    AudioInput, SttCapabilities, SttOptions, SttProvider, Transcript, transcript_from_provider_text,
};
use crate::providers::{ProviderError, http};
use base64::Engine;

const RESPONSE_BODY_LIMIT_CHARS: usize = 2_048;

pub struct MimoStt {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl MimoStt {
    pub fn new(
        client: reqwest::Client,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    fn build_body<'a>(&'a self, audio: &AudioInput, opts: &'a SttOptions) -> MimoRequest<'a> {
        let language = match opts.language.as_deref().map(str::trim) {
            Some(language) if !language.is_empty() && language != "auto" => language,
            _ => "auto",
        };
        let wav = base64::engine::general_purpose::STANDARD.encode(&audio.wav_16k_mono);
        MimoRequest {
            model: &self.model,
            messages: [MimoUserMessage {
                role: "user",
                content: [MimoAudioContent {
                    kind: "input_audio",
                    input_audio: MimoInputAudio {
                        data: format!("data:audio/wav;base64,{wav}"),
                    },
                }],
            }],
            stream: false,
            asr_options: MimoAsrOptions { language },
        }
    }
}

#[derive(serde::Serialize)]
struct MimoRequest<'a> {
    model: &'a str,
    messages: [MimoUserMessage; 1],
    stream: bool,
    asr_options: MimoAsrOptions<'a>,
}

#[derive(serde::Serialize)]
struct MimoUserMessage {
    role: &'static str,
    content: [MimoAudioContent; 1],
}

#[derive(serde::Serialize)]
struct MimoAudioContent {
    #[serde(rename = "type")]
    kind: &'static str,
    input_audio: MimoInputAudio,
}

#[derive(serde::Serialize)]
struct MimoInputAudio {
    data: String,
}

#[derive(serde::Serialize)]
struct MimoAsrOptions<'a> {
    language: &'a str,
}

#[derive(serde::Deserialize)]
struct MimoResponse {
    choices: Option<Vec<MimoChoice>>,
}

#[derive(serde::Deserialize)]
struct MimoChoice {
    message: Option<MimoResponseMessage>,
}

#[derive(serde::Deserialize)]
struct MimoResponseMessage {
    content: Option<serde_json::Value>,
}

fn response_error(reason: &str, body: &str) -> ProviderError {
    ProviderError::InvalidRequest(format!("MiMo {reason}; body: {}", truncated_body(body)))
}

fn truncated_body(body: &str) -> String {
    let mut chars = body.chars();
    let truncated: String = chars.by_ref().take(RESPONSE_BODY_LIMIT_CHARS).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn parse_response(body: &str) -> Result<Transcript, ProviderError> {
    let parsed: MimoResponse = serde_json::from_str(body)
        .map_err(|error| response_error(&format!("响应 JSON 解析失败: {error}"), body))?;
    let choices = parsed
        .choices
        .ok_or_else(|| response_error("响应 choices 缺失", body))?;
    let choice = choices
        .first()
        .ok_or_else(|| response_error("响应 choices 为空", body))?;
    let message = choice
        .message
        .as_ref()
        .ok_or_else(|| response_error("响应 message 缺失", body))?;
    let content = message
        .content
        .as_ref()
        .ok_or_else(|| response_error("响应 message.content 缺失", body))?
        .as_str()
        .ok_or_else(|| response_error("响应 message.content 不是字符串", body))?;
    Ok(transcript_from_provider_text(content, None))
}

#[async_trait::async_trait]
impl SttProvider for MimoStt {
    async fn transcribe(
        &self,
        audio: AudioInput,
        opts: SttOptions,
    ) -> Result<Transcript, ProviderError> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = self.build_body(&audio, &opts);
        http::with_retry(|| async {
            let response = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await
                .map_err(ProviderError::from_reqwest)?;
            let status = response.status().as_u16();
            let text = response.text().await.map_err(ProviderError::from_reqwest)?;
            if status >= 400 {
                return Err(ProviderError::from_status(status, text));
            }
            parse_response(&text)
        })
        .await
    }

    fn capabilities(&self) -> SttCapabilities {
        SttCapabilities {
            max_bytes: None,
            supports_prompt: false,
            supports_language: true,
        }
    }
}
