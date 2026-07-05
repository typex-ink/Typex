//! SttProvider trait（03 §2）。
pub mod openai_compat;
pub mod volcengine;

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
    async fn transcribe(
        &self,
        audio: AudioInput,
        opts: SttOptions,
    ) -> Result<Transcript, ProviderError>;
    fn capabilities(&self) -> SttCapabilities;
}

/// 长录音自动切片转写（02 F-1 无时长硬上限）：
/// 超过 provider 单次上限时在 VAD 静音处切片，分段转写后拼接，用户无感。
pub async fn transcribe_auto_chunk(
    provider: &dyn SttProvider,
    audio: AudioInput,
    opts: SttOptions,
) -> Result<Transcript, ProviderError> {
    let max = provider.capabilities().max_bytes;
    let Some(max_bytes) = max else {
        return provider.transcribe(audio, opts).await;
    };
    if audio.wav_16k_mono.len() <= max_bytes {
        return provider.transcribe(audio, opts).await;
    }

    // 解 WAV → 采样 → 静音处切片
    let reader = hound::WavReader::new(std::io::Cursor::new(&audio.wav_16k_mono))
        .map_err(|e| ProviderError::InvalidRequest(format!("WAV 解析失败: {e}")))?;
    let samples: Vec<f32> = reader
        .into_samples::<i16>()
        .filter_map(|s| s.ok())
        .map(|s| s as f32 / i16::MAX as f32)
        .collect();
    // 16-bit PCM：每采样 2 字节 + 头部余量
    let max_samples = (max_bytes.saturating_sub(1024)) / 2;
    let chunks = crate::audio::vad::split_at_silence(&samples, max_samples);

    let mut full_text = String::new();
    let mut detected = None;
    for (start, end) in chunks {
        let wav = crate::audio::pipeline::to_wav_16k_mono(&samples[start..end], 16_000)
            .map_err(|e| ProviderError::InvalidRequest(e.message))?;
        let duration_ms = ((end - start) as u64 * 1000) / 16_000;
        let t = provider
            .transcribe(
                AudioInput {
                    wav_16k_mono: wav,
                    duration_ms,
                },
                opts.clone(),
            )
            .await?;
        if !full_text.is_empty() && !t.text.is_empty() {
            full_text.push(' ');
        }
        full_text.push_str(t.text.trim());
        detected = detected.or(t.detected_language);
    }
    Ok(Transcript {
        text: full_text,
        detected_language: detected,
    })
}
