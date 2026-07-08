//! 本地 STT Provider · Whisper large-v3 高配精度档（sherpa-onnx）。
//!
//! 该路径复用 sherpa-onnx 的 Whisper offline recognizer，加载 ONNX
//! encoder/decoder + tokens。当前 sherpa-rs 默认 CPU provider，因此它是
//! 手动选择的高精度档，不参与零配置硬件分档。

use crate::providers::ProviderError;
use crate::providers::stt::{AudioInput, SttCapabilities, SttOptions, SttProvider, Transcript};
use sherpa_rs::whisper::{WhisperConfig, WhisperRecognizer};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct WhisperStt {
    recognizer: Mutex<Option<WhisperRecognizer>>,
    encoder_path: PathBuf,
    decoder_path: PathBuf,
    tokens_path: PathBuf,
    num_threads: i32,
}

impl WhisperStt {
    /// `model_dir` = `{data_dir}/models/whisper-large-v3-int8/`。
    pub fn new(model_dir: PathBuf, num_threads: i32) -> Self {
        Self::from_files(
            model_dir.join("large-v3-encoder.int8.onnx"),
            model_dir.join("large-v3-decoder.int8.onnx"),
            model_dir.join("large-v3-tokens.txt"),
            num_threads,
        )
    }

    pub fn from_files(
        encoder_path: PathBuf,
        decoder_path: PathBuf,
        tokens_path: PathBuf,
        num_threads: i32,
    ) -> Self {
        Self {
            recognizer: Mutex::new(None),
            encoder_path,
            decoder_path,
            tokens_path,
            num_threads,
        }
    }

    fn ensure_loaded(&self) -> Result<(), ProviderError> {
        let mut guard = self.recognizer.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
        if !self.encoder_path.exists() || !self.decoder_path.exists() || !self.tokens_path.exists()
        {
            return Err(ProviderError::InvalidRequest(
                "模型未下载：请在设置-模型服务中下载 Whisper large-v3".into(),
            ));
        }

        let recognizer = WhisperRecognizer::new(WhisperConfig {
            encoder: self.encoder_path.display().to_string(),
            decoder: self.decoder_path.display().to_string(),
            tokens: self.tokens_path.display().to_string(),
            language: String::new(),
            provider: Some("cpu".into()),
            num_threads: Some(self.num_threads),
            tail_paddings: Some(-1),
            debug: false,
            bpe_vocab: None,
        })
        .map_err(|e| ProviderError::InvalidRequest(format!("Whisper 模型加载失败: {e}")))?;
        *guard = Some(recognizer);
        Ok(())
    }

    /// 释放常驻内存（运行时策略切换用）。
    pub fn unload(&self) {
        *self.recognizer.lock().unwrap() = None;
    }
}

fn wav_to_samples(wav: &[u8]) -> Result<Vec<f32>, ProviderError> {
    let reader = hound::WavReader::new(std::io::Cursor::new(wav))
        .map_err(|e| ProviderError::InvalidRequest(format!("WAV 解析失败: {e}")))?;
    Ok(reader
        .into_samples::<i16>()
        .filter_map(|s| s.ok())
        .map(|s| s as f32 / i16::MAX as f32)
        .collect())
}

#[async_trait::async_trait]
impl SttProvider for WhisperStt {
    async fn transcribe(
        &self,
        audio: AudioInput,
        _opts: SttOptions,
    ) -> Result<Transcript, ProviderError> {
        let samples = wav_to_samples(&audio.wav_16k_mono)?;
        self.ensure_loaded()?;
        let mut guard = self.recognizer.lock().unwrap();
        let recognizer = guard.as_mut().expect("ensure_loaded 已初始化");
        let result = recognizer.transcribe(16_000, &samples);
        Ok(Transcript {
            text: result.text.trim().to_string(),
            detected_language: (!result.lang.is_empty()).then_some(result.lang),
        })
    }

    fn capabilities(&self) -> SttCapabilities {
        SttCapabilities {
            max_bytes: None,
            supports_prompt: false,
            supports_language: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_invalid_request_not_panic() {
        let stt = WhisperStt::new(PathBuf::from("/nonexistent/whisper"), 4);
        let err = stt.ensure_loaded().unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
        assert!(err.to_string().contains("模型未下载"));
    }

    #[test]
    fn capabilities_are_accuracy_model_defaults() {
        let stt = WhisperStt::new(PathBuf::from("/tmp/whisper"), 4);
        let caps = stt.capabilities();
        assert_eq!(caps.max_bytes, None);
        assert!(!caps.supports_prompt);
        assert!(!caps.supports_language);
    }
}
