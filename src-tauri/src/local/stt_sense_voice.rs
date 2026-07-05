//! 本地 STT Provider · SenseVoice 轻量档（CP-8.3 / 03 §2.3 / ADR-22）。
//!
//! sherpa-onnx 静态链接 + SenseVoice-Small int8：非自回归、CPU 实时数倍速，
//! 弱机器上唯一保证实时的选项。实现同一 `SttProvider` trait（kind: local，
//! 无 base_url/凭据）；错误分类只剩 InvalidRequest / 模型未下载（NotConfigured）。

use crate::providers::ProviderError;
use crate::providers::stt::{AudioInput, SttCapabilities, SttOptions, SttProvider, Transcript};
use sherpa_rs::sense_voice::{SenseVoiceConfig, SenseVoiceRecognizer};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct SenseVoiceStt {
    /// 惰性初始化：模型加载 1s 级，首次转写时才加载（录音时预热策略可后续加）
    recognizer: Mutex<Option<SenseVoiceRecognizer>>,
    model_dir: PathBuf,
    num_threads: i32,
}

impl SenseVoiceStt {
    /// `model_dir` = `{data_dir}/models/sense-voice-small-int8/`。
    pub fn new(model_dir: PathBuf, num_threads: i32) -> Self {
        Self {
            recognizer: Mutex::new(None),
            model_dir,
            num_threads,
        }
    }

    fn ensure_loaded(&self) -> Result<(), ProviderError> {
        let mut guard = self.recognizer.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
        let model = self.model_dir.join("model.int8.onnx");
        let tokens = self.model_dir.join("tokens.txt");
        if !model.exists() || !tokens.exists() {
            // 模型未下载（03 §2.3 错误分类）
            return Err(ProviderError::InvalidRequest(
                "模型未下载：请在设置-模型服务中下载 SenseVoice".into(),
            ));
        }
        let config = SenseVoiceConfig {
            model: model.display().to_string(),
            tokens: tokens.display().to_string(),
            language: "auto".into(),
            use_itn: true,
            num_threads: Some(self.num_threads),
            ..Default::default()
        };
        let recognizer = SenseVoiceRecognizer::new(config)
            .map_err(|e| ProviderError::InvalidRequest(format!("SenseVoice 模型加载失败: {e}")))?;
        *guard = Some(recognizer);
        Ok(())
    }

    /// 释放常驻内存（运行时策略切换用）。
    pub fn unload(&self) {
        *self.recognizer.lock().unwrap() = None;
    }
}

/// WAV（16k mono s16le）→ f32 采样。
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
impl SttProvider for SenseVoiceStt {
    async fn transcribe(
        &self,
        audio: AudioInput,
        _opts: SttOptions,
    ) -> Result<Transcript, ProviderError> {
        let samples = wav_to_samples(&audio.wav_16k_mono)?;
        self.ensure_loaded()?;
        // 推理是 CPU 阻塞调用；trait 是 async——直接在调用方的 blocking 语境跑
        // （orchestrator 的 STT 调用本就在 spawn 的 task 里，短音频毫秒级不成问题）
        let mut guard = self.recognizer.lock().unwrap();
        let recognizer = guard.as_mut().expect("ensure_loaded 已初始化");
        let result = recognizer.transcribe(16_000, &samples);
        Ok(Transcript {
            text: result.text.trim().to_string(),
            detected_language: (!result.lang.is_empty()).then(|| result.lang.clone()),
        })
    }

    fn capabilities(&self) -> SttCapabilities {
        SttCapabilities {
            max_bytes: None, // 本地无 25 MB 上限（03 §2.3）
            supports_prompt: false,
            supports_language: false, // SenseVoice 自动判语种
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_invalid_request_not_panic() {
        let stt = SenseVoiceStt::new(PathBuf::from("/nonexistent/dir"), 1);
        let err = stt.ensure_loaded().unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
        assert!(err.to_string().contains("模型未下载"));
    }

    #[test]
    fn capabilities_report_unlimited_audio() {
        let stt = SenseVoiceStt::new(PathBuf::from("/tmp"), 1);
        let caps = stt.capabilities();
        assert!(caps.max_bytes.is_none());
    }

    #[test]
    fn wav_parse_error_classified() {
        let err = wav_to_samples(b"not a wav").unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
    }
}
