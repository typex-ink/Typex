//! 本地 STT Provider · NeMo Transducer / TDT 高速英欧语档（sherpa-onnx）。
//!
//! 复用 `sherpa-rs` 的 `TransducerRecognizer`（`model_type = "nemo_transducer"`，
//! `decoding_method = "greedy_search"`），加载 Parakeet TDT 的 ONNX
//! encoder/decoder/joiner + tokens。当前走 CPU provider，不参与零配置硬件分档。

use crate::providers::ProviderError;
use crate::providers::stt::{
    AudioInput, NativeSttJobGate, SttCapabilities, SttOptions, SttProvider, Transcript,
};
use sherpa_rs::transducer::{TransducerConfig, TransducerRecognizer};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct TransducerStt {
    recognizer: Arc<Mutex<Option<TransducerRecognizer>>>,
    native_jobs: NativeSttJobGate,
    encoder_path: PathBuf,
    decoder_path: PathBuf,
    joiner_path: PathBuf,
    tokens_path: PathBuf,
    num_threads: i32,
}

impl TransducerStt {
    pub fn from_files(
        encoder_path: PathBuf,
        decoder_path: PathBuf,
        joiner_path: PathBuf,
        tokens_path: PathBuf,
        num_threads: i32,
    ) -> Self {
        Self {
            recognizer: Arc::new(Mutex::new(None)),
            native_jobs: NativeSttJobGate::new(),
            encoder_path,
            decoder_path,
            joiner_path,
            tokens_path,
            num_threads,
        }
    }

    #[cfg(test)]
    pub fn new(model_dir: PathBuf, num_threads: i32) -> Self {
        Self::from_files(
            model_dir.join("encoder.int8.onnx"),
            model_dir.join("decoder.int8.onnx"),
            model_dir.join("joiner.int8.onnx"),
            model_dir.join("tokens.txt"),
            num_threads,
        )
    }

    #[cfg(test)]
    fn ensure_loaded(&self) -> Result<(), ProviderError> {
        ensure_recognizer_loaded(
            &self.recognizer,
            &self.encoder_path,
            &self.decoder_path,
            &self.joiner_path,
            &self.tokens_path,
            self.num_threads,
        )
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

fn validate_transducer_files(
    encoder_path: &Path,
    decoder_path: &Path,
    joiner_path: &Path,
    tokens_path: &Path,
) -> Result<(), ProviderError> {
    for path in [encoder_path, decoder_path, joiner_path, tokens_path] {
        let Ok(meta) = std::fs::metadata(path) else {
            return Err(ProviderError::InvalidRequest(
                "模型未下载：请在设置-模型服务中下载 Parakeet 本地语音模型".into(),
            ));
        };
        if !meta.is_file() || meta.len() == 0 {
            return Err(ProviderError::InvalidRequest(
                "Parakeet 模型文件不完整：请删除后重新下载".into(),
            ));
        }
    }
    // OfflineRecognizerTransducerNeMoImpl::PostInit 在缺失 <blk> 时会触发 C++ exit(-1)，
    // 此处提前做轻量文本预校验并转为可恢复的 InvalidRequest。
    let tokens_text = std::fs::read_to_string(tokens_path)
        .map_err(|_| ProviderError::InvalidRequest("Parakeet tokens.txt 无法读取".into()))?;
    let has_blk = tokens_text
        .lines()
        .any(|line| line.split_whitespace().next() == Some("<blk>"));
    if !has_blk {
        return Err(ProviderError::InvalidRequest(
            "Parakeet tokens.txt 无效：缺少 <blk> 词条".into(),
        ));
    }
    Ok(())
}

#[async_trait::async_trait]
impl SttProvider for TransducerStt {
    async fn transcribe(
        &self,
        audio: AudioInput,
        _opts: SttOptions,
    ) -> Result<Transcript, ProviderError> {
        let recognizer = Arc::clone(&self.recognizer);
        let encoder_path = self.encoder_path.clone();
        let decoder_path = self.decoder_path.clone();
        let joiner_path = self.joiner_path.clone();
        let tokens_path = self.tokens_path.clone();
        let num_threads = self.num_threads;
        self.native_jobs
            .run("Transducer 转写任务", move || {
                let samples = wav_to_samples(&audio.wav_16k_mono)?;
                ensure_recognizer_loaded(
                    &recognizer,
                    &encoder_path,
                    &decoder_path,
                    &joiner_path,
                    &tokens_path,
                    num_threads,
                )?;
                let mut guard = recognizer.lock().unwrap();
                let recognizer = guard.as_mut().expect("ensure_loaded 已初始化");
                let text = recognizer.transcribe(16_000, &samples);
                Ok(Transcript {
                    text: text.trim().to_string(),
                    detected_language: None,
                })
            })
            .await
    }

    fn capabilities(&self) -> SttCapabilities {
        SttCapabilities {
            max_bytes: None,
            supports_prompt: false,
            supports_language: false,
        }
    }
}

fn ensure_recognizer_loaded(
    recognizer_slot: &Mutex<Option<TransducerRecognizer>>,
    encoder_path: &Path,
    decoder_path: &Path,
    joiner_path: &Path,
    tokens_path: &Path,
    num_threads: i32,
) -> Result<(), ProviderError> {
    let mut guard = recognizer_slot.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }
    validate_transducer_files(encoder_path, decoder_path, joiner_path, tokens_path)?;

    let recognizer = TransducerRecognizer::new(TransducerConfig {
        encoder: encoder_path.display().to_string(),
        decoder: decoder_path.display().to_string(),
        joiner: joiner_path.display().to_string(),
        tokens: tokens_path.display().to_string(),
        model_type: "nemo_transducer".into(),
        decoding_method: "greedy_search".into(),
        sample_rate: 16_000,
        feature_dim: 80,
        num_threads,
        provider: Some("cpu".into()),
        ..TransducerConfig::default()
    })
    .map_err(|e| ProviderError::InvalidRequest(format!("Parakeet 模型加载失败: {e}")))?;
    *guard = Some(recognizer);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_invalid_request_not_panic() {
        let stt = TransducerStt::new(PathBuf::from("/nonexistent/parakeet"), 4);
        let err = stt.ensure_loaded().unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
        assert!(err.to_string().contains("模型未下载"));
    }

    #[test]
    fn empty_or_blk_missing_tokens_rejected_before_native_init() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx"] {
            std::fs::write(dir.path().join(name), b"onnx").unwrap();
        }
        std::fs::write(dir.path().join("tokens.txt"), "a 0\nb 1\n").unwrap();

        let stt = TransducerStt::new(dir.path().to_path_buf(), 2);
        let err = stt.ensure_loaded().unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
        assert!(err.to_string().contains("<blk>"));
    }

    #[test]
    fn capabilities_report_local_transducer_defaults() {
        let stt = TransducerStt::new(PathBuf::from("/tmp/parakeet"), 4);
        let caps = stt.capabilities();
        assert_eq!(caps.max_bytes, None);
        assert!(!caps.supports_prompt);
        assert!(!caps.supports_language);
    }
}
