//! openai_compat STT（03 §2.1）：multipart POST {base_url}/audio/transcriptions。
//! 覆盖 OpenAI / Groq / SiliconFlow / 自建（vLLM、speaches…）。

use super::{AudioInput, SttCapabilities, SttOptions, SttProvider, Transcript};
use crate::providers::{http, ProviderError};
use std::collections::HashMap;

pub struct OpenAiCompatStt {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    extra_headers: HashMap<String, String>,
    extra_form: HashMap<String, String>,
}

impl OpenAiCompatStt {
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
            extra_headers: HashMap::new(),
            extra_form: HashMap::new(),
        }
    }

    pub fn with_extras(
        mut self,
        headers: HashMap<String, String>,
        form: HashMap<String, String>,
    ) -> Self {
        self.extra_headers = headers;
        self.extra_form = form;
        self
    }

    fn build_form(&self, audio: &AudioInput, opts: &SttOptions) -> reqwest::multipart::Form {
        let part = reqwest::multipart::Part::bytes(audio.wav_16k_mono.clone())
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .expect("static mime");
        let mut form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model", self.model.clone())
            .text("response_format", "json");
        if let Some(lang) = &opts.language {
            if lang != "auto" && !lang.is_empty() {
                form = form.text("language", lang.clone());
            }
        }
        if let Some(prompt) = &opts.prompt {
            form = form.text("prompt", prompt.clone());
        }
        if let Some(t) = opts.temperature {
            form = form.text("temperature", t.to_string());
        }
        for (k, v) in &self.extra_form {
            form = form.text(k.clone(), v.clone());
        }
        form
    }
}

#[derive(serde::Deserialize)]
struct TranscriptionResponse {
    text: String,
    #[serde(default)]
    language: Option<String>,
}

#[async_trait::async_trait]
impl SttProvider for OpenAiCompatStt {
    async fn transcribe(
        &self,
        audio: AudioInput,
        opts: SttOptions,
    ) -> Result<Transcript, ProviderError> {
        let url = format!("{}/audio/transcriptions", self.base_url);
        http::with_retry(|| async {
            let mut req = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .multipart(self.build_form(&audio, &opts));
            for (k, v) in &self.extra_headers {
                req = req.header(k, v);
            }
            let resp = req.send().await.map_err(ProviderError::from_reqwest)?;
            let status = resp.status().as_u16();
            let body = resp.text().await.map_err(ProviderError::from_reqwest)?;
            if status >= 400 {
                return Err(ProviderError::from_status(status, body));
            }
            let parsed: TranscriptionResponse = serde_json::from_str(&body)
                .map_err(|e| ProviderError::InvalidRequest(format!("响应解析失败: {e}; body: {body}")))?;
            Ok(Transcript { text: parsed.text, detected_language: parsed.language })
        })
        .await
    }

    fn capabilities(&self) -> SttCapabilities {
        SttCapabilities {
            max_bytes: Some(25 * 1024 * 1024), // OpenAI/Groq 25 MB 上限（03 §2.1）
            supports_prompt: true,
            supports_language: true,
        }
    }
}
