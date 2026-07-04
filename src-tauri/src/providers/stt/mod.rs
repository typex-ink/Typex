//! SttProvider trait（03 §2）。
pub mod openai_compat;

use super::ProviderError;

/// 内部统一喂 16 kHz mono WAV（03 §2）。
#[derive(Debug, Clone)]
pub struct AudioInput {
    pub wav_16k_mono: Vec<u8>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SttOptions {
    /// ISO-639-1；None = 自动检测
    pub language: Option<String>,
    /// 术语引导（F-10 个人词典入口）
    pub prompt: Option<String>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    pub detected_language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SttCapabilities {
    /// 单次请求最大音频字节数（切片阈值；None = 无上限）
    pub max_bytes: Option<usize>,
    pub supports_prompt: bool,
    pub supports_language: bool,
}

#[async_trait::async_trait]
pub trait SttProvider: Send + Sync {
    async fn transcribe(&self, audio: AudioInput, opts: SttOptions)
        -> Result<Transcript, ProviderError>;
    fn capabilities(&self) -> SttCapabilities;
}
