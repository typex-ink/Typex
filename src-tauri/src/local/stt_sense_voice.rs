//! 本地 STT Provider · SenseVoice 轻量档（03 §2.3 / ADR-22）。
//!
//! sherpa-onnx 静态链接 + SenseVoice-Small int8：非自回归、CPU 实时数倍速，
//! 弱机器上唯一保证实时的选项。实现同一 `SttProvider` trait（kind: local，
//! 无 base_url/凭据）；错误分类只剩 InvalidRequest / 模型未下载（NotConfigured）。

use crate::providers::ProviderError;
use crate::providers::stt::{AudioInput, SttCapabilities, SttOptions, SttProvider, Transcript};
use std::collections::hash_map::DefaultHasher;
use std::ffi::{CStr, CString};
use std::hash::{Hash, Hasher};
use std::mem;
use std::path::PathBuf;
use std::sync::Mutex;

struct SenseVoiceRecognizer {
    recognizer: *const sherpa_rs_sys::SherpaOnnxOfflineRecognizer,
}

unsafe impl Send for SenseVoiceRecognizer {}
unsafe impl Sync for SenseVoiceRecognizer {}

impl SenseVoiceRecognizer {
    fn new(
        model: &str,
        tokens: &str,
        num_threads: i32,
        hotwords_file: Option<&str>,
    ) -> Result<Self, ProviderError> {
        let model = cstring(model)?;
        let tokens = cstring(tokens)?;
        let provider = cstring("cpu")?;
        let language = cstring("auto")?;
        let hotwords_file = cstring(hotwords_file.unwrap_or(""))?;
        let sense_voice_config = sherpa_rs_sys::SherpaOnnxOfflineSenseVoiceModelConfig {
            model: model.as_ptr(),
            language: language.as_ptr(),
            use_itn: 1,
        };
        let model_config = unsafe {
            sherpa_rs_sys::SherpaOnnxOfflineModelConfig {
                tokens: tokens.as_ptr(),
                provider: provider.as_ptr(),
                num_threads,
                debug: 0,
                sense_voice: sense_voice_config,
                bpe_vocab: mem::zeroed(),
                model_type: mem::zeroed(),
                modeling_unit: mem::zeroed(),
                nemo_ctc: mem::zeroed(),
                paraformer: mem::zeroed(),
                tdnn: mem::zeroed(),
                telespeech_ctc: mem::zeroed(),
                fire_red_asr: mem::zeroed(),
                transducer: mem::zeroed(),
                whisper: mem::zeroed(),
                moonshine: mem::zeroed(),
                dolphin: mem::zeroed(),
                zipformer_ctc: mem::zeroed(),
                canary: mem::zeroed(),
            }
        };
        let config = unsafe {
            sherpa_rs_sys::SherpaOnnxOfflineRecognizerConfig {
                decoding_method: mem::zeroed(),
                feat_config: sherpa_rs_sys::SherpaOnnxFeatureConfig {
                    sample_rate: 16_000,
                    feature_dim: 80,
                },
                hotwords_file: hotwords_file.as_ptr(),
                hotwords_score: if hotwords_file.as_bytes().is_empty() {
                    0.0
                } else {
                    2.0
                },
                lm_config: sherpa_rs_sys::SherpaOnnxOfflineLMConfig {
                    model: mem::zeroed(),
                    scale: 0.0,
                },
                max_active_paths: 0,
                model_config,
                rule_fars: mem::zeroed(),
                rule_fsts: mem::zeroed(),
                blank_penalty: 0.0,
                hr: mem::zeroed(),
            }
        };
        let recognizer = unsafe { sherpa_rs_sys::SherpaOnnxCreateOfflineRecognizer(&config) };
        if recognizer.is_null() {
            return Err(ProviderError::InvalidRequest(
                "SenseVoice 模型加载失败".into(),
            ));
        }
        Ok(Self { recognizer })
    }

    fn transcribe(
        &mut self,
        sample_rate: u32,
        samples: &[f32],
    ) -> Result<Transcript, ProviderError> {
        unsafe {
            let stream = sherpa_rs_sys::SherpaOnnxCreateOfflineStream(self.recognizer);
            if stream.is_null() {
                return Err(ProviderError::InvalidRequest(
                    "SenseVoice stream 创建失败".into(),
                ));
            }
            sherpa_rs_sys::SherpaOnnxAcceptWaveformOffline(
                stream,
                sample_rate as i32,
                samples.as_ptr(),
                samples.len().try_into().unwrap(),
            );
            sherpa_rs_sys::SherpaOnnxDecodeOfflineStream(self.recognizer, stream);
            let result_ptr = sherpa_rs_sys::SherpaOnnxGetOfflineStreamResult(stream);
            if result_ptr.is_null() {
                sherpa_rs_sys::SherpaOnnxDestroyOfflineStream(stream);
                return Err(ProviderError::InvalidRequest(
                    "SenseVoice result 为空".into(),
                ));
            }
            let raw = result_ptr.read();
            let text = cstr_to_string(raw.text);
            let lang = cstr_to_string(raw.lang);
            sherpa_rs_sys::SherpaOnnxDestroyOfflineRecognizerResult(result_ptr);
            sherpa_rs_sys::SherpaOnnxDestroyOfflineStream(stream);
            Ok(Transcript {
                text: text.trim().to_string(),
                detected_language: (!lang.is_empty()).then_some(lang),
            })
        }
    }
}

impl Drop for SenseVoiceRecognizer {
    fn drop(&mut self) {
        unsafe {
            sherpa_rs_sys::SherpaOnnxDestroyOfflineRecognizer(self.recognizer);
        }
    }
}

struct LoadedSenseVoice {
    recognizer: SenseVoiceRecognizer,
    hotwords_key: Option<String>,
    hotwords_path: Option<PathBuf>,
}

impl Drop for LoadedSenseVoice {
    fn drop(&mut self) {
        if let Some(path) = &self.hotwords_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub struct SenseVoiceStt {
    /// 惰性初始化：模型加载 1s 级，首次转写时才加载（录音时预热策略可后续加）
    recognizer: Mutex<Option<LoadedSenseVoice>>,
    model_path: PathBuf,
    tokens_path: PathBuf,
    num_threads: i32,
}

impl SenseVoiceStt {
    /// `model_dir` = `{data_dir}/models/sense-voice-small-int8/`。
    pub fn new(model_dir: PathBuf, num_threads: i32) -> Self {
        Self::from_files(
            model_dir.join("model.int8.onnx"),
            model_dir.join("tokens.txt"),
            num_threads,
        )
    }

    /// 导入模型使用：文件名由用户清单决定。
    pub fn from_files(model_path: PathBuf, tokens_path: PathBuf, num_threads: i32) -> Self {
        Self {
            recognizer: Mutex::new(None),
            model_path,
            tokens_path,
            num_threads,
        }
    }

    fn ensure_loaded(&self, hotwords: Option<&str>) -> Result<(), ProviderError> {
        let hotwords_key = hotwords
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let mut guard = self.recognizer.lock().unwrap();
        if guard
            .as_ref()
            .is_some_and(|loaded| loaded.hotwords_key == hotwords_key)
        {
            return Ok(());
        }
        let model = &self.model_path;
        let tokens = &self.tokens_path;
        if !model.exists() || !tokens.exists() {
            // 模型未下载（03 §2.3 错误分类）
            return Err(ProviderError::InvalidRequest(
                "模型未下载：请在设置-模型服务中下载 SenseVoice".into(),
            ));
        }
        let (hotwords_path, hotwords_file) = write_hotwords_file(hotwords_key.as_deref())?;
        let recognizer = SenseVoiceRecognizer::new(
            &model.display().to_string(),
            &tokens.display().to_string(),
            self.num_threads,
            hotwords_file.as_deref(),
        )?;
        *guard = Some(LoadedSenseVoice {
            recognizer,
            hotwords_key,
            hotwords_path,
        });
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
        opts: SttOptions,
    ) -> Result<Transcript, ProviderError> {
        let samples = wav_to_samples(&audio.wav_16k_mono)?;
        self.ensure_loaded(opts.prompt.as_deref())?;
        // 推理是 CPU 阻塞调用；trait 是 async——直接在调用方的 blocking 语境跑
        // （orchestrator 的 STT 调用本就在 spawn 的 task 里，短音频毫秒级不成问题）
        let mut guard = self.recognizer.lock().unwrap();
        let loaded = guard.as_mut().expect("ensure_loaded 已初始化");
        loaded.recognizer.transcribe(16_000, &samples)
    }

    fn capabilities(&self) -> SttCapabilities {
        SttCapabilities {
            max_bytes: None, // 本地无 25 MB 上限（03 §2.3）
            supports_prompt: true,
            supports_language: false, // SenseVoice 自动判语种
        }
    }
}

fn cstring(value: &str) -> Result<CString, ProviderError> {
    CString::new(value).map_err(|_| ProviderError::InvalidRequest("字符串含 NUL 字节".into()))
}

unsafe fn cstr_to_string(ptr: *const std::ffi::c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
    }
}

fn write_hotwords_file(
    hotwords: Option<&str>,
) -> Result<(Option<PathBuf>, Option<String>), ProviderError> {
    let Some(content) = hotwords_file_content(hotwords) else {
        return Ok((None, None));
    };
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let path = std::env::temp_dir().join(format!(
        "typex-sensevoice-hotwords-{}-{:x}.txt",
        std::process::id(),
        hasher.finish()
    ));
    std::fs::write(&path, content)
        .map_err(|e| ProviderError::InvalidRequest(format!("写入 hotwords 文件失败: {e}")))?;
    let display = path.display().to_string();
    Ok((Some(path), Some(display)))
}

fn hotwords_file_content(hotwords: Option<&str>) -> Option<String> {
    let words: Vec<String> = hotwords?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    (!words.is_empty()).then(|| words.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_invalid_request_not_panic() {
        let stt = SenseVoiceStt::new(PathBuf::from("/nonexistent/dir"), 1);
        let err = stt.ensure_loaded(None).unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
        assert!(err.to_string().contains("模型未下载"));
    }

    #[test]
    fn capabilities_report_unlimited_audio() {
        let stt = SenseVoiceStt::new(PathBuf::from("/tmp"), 1);
        let caps = stt.capabilities();
        assert!(caps.max_bytes.is_none());
        assert!(caps.supports_prompt);
    }

    #[test]
    fn wav_parse_error_classified() {
        let err = wav_to_samples(b"not a wav").unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
    }

    #[test]
    fn hotwords_file_content_trims_empty_lines() {
        assert_eq!(
            hotwords_file_content(Some(" Typex \n\nOpenAI ")).unwrap(),
            "Typex\nOpenAI"
        );
        assert!(hotwords_file_content(Some("  \n")).is_none());
        assert!(hotwords_file_content(None).is_none());
    }
}
