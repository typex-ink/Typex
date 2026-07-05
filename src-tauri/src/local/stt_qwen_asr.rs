//! 本地 STT Provider · Qwen3-ASR 标准/性能档（CP-8.4 / 03 §2.3 / ADR-22）。
//!
//! llama.cpp mtmd（多模态，experimental）跑 Qwen3-ASR GGUF：16k mono WAV →
//! f32 采样 → mtmd 音频 bitmap → encode → 自回归解码转写。除主模型 GGUF 外
//! 还需 mmproj（音频编码器投影）文件——两者都由模型下载管理器落盘。
//!
//! llama.cpp 长音频有已知 bug（03 §2.3 / ADR-22 工程注意）：`capabilities()`
//! 报保守的 max_bytes，上层 `transcribe_auto_chunk` 会在 VAD 静音处切片规避。

use crate::local::llm_llama::llama_backend;
use crate::providers::ProviderError;
use crate::providers::stt::{AudioInput, SttCapabilities, SttOptions, SttProvider, Transcript};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
use llama_cpp_2::mtmd::{
    MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText, mtmd_default_marker,
};
use llama_cpp_2::sampling::LlamaSampler;
use std::ffi::CString;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// 单次请求最大音频字节数（16k mono s16le ≈ 32KB/s，10MB ≈ 5 分钟）。
/// 保守值：强制上层 VAD 切片，规避 llama.cpp 长音频 bug（ADR-22）。
const MAX_AUDIO_BYTES: usize = 10 * 1024 * 1024;
/// 上下文窗口：音频 embedding + 转写文本；短分段（VAD 切片后）足够。
const N_CTX: u32 = 4096;
/// 转写输出 token 上限（防跑飞；5 分钟语音的文字远小于此）。
const MAX_NEW_TOKENS: u32 = 2048;

#[derive(Debug)]
struct LoadedAsr {
    model: LlamaModel,
    mtmd: MtmdContext,
}

pub struct QwenAsrStt {
    /// 惰性初始化：模型加载秒级，首次转写时才加载。
    state: Mutex<Option<Arc<LoadedAsr>>>,
    model_path: PathBuf,
    mmproj_path: PathBuf,
    n_threads: i32,
}

impl QwenAsrStt {
    /// `model_path` = 主模型 GGUF，`mmproj_path` = 音频编码器投影 GGUF
    /// （均在 `{data_dir}/models/{model_id}/` 下，构造函数注入）。
    pub fn new(model_path: PathBuf, mmproj_path: PathBuf, n_threads: i32) -> Self {
        Self {
            state: Mutex::new(None),
            model_path,
            mmproj_path,
            n_threads,
        }
    }

    /// 释放常驻内存（运行时策略切换用）。进行中的转写持有自己的 Arc，不受影响。
    pub fn unload(&self) {
        *self.state.lock().unwrap() = None;
    }

    fn ensure_loaded(&self) -> Result<Arc<LoadedAsr>, ProviderError> {
        let mut guard = self.state.lock().unwrap();
        if let Some(loaded) = guard.as_ref() {
            return Ok(Arc::clone(loaded));
        }
        if !self.model_path.exists() || !self.mmproj_path.exists() {
            // 模型未下载（03 §2.3 错误分类）
            return Err(ProviderError::InvalidRequest(
                "模型未下载：请在设置-模型服务中下载 Qwen3-ASR".into(),
            ));
        }
        let invalid = |e: &dyn std::fmt::Display| {
            ProviderError::InvalidRequest(format!("Qwen3-ASR 模型加载失败: {e}"))
        };
        let model = LlamaModel::load_from_file(
            llama_backend(),
            &self.model_path,
            &LlamaModelParams::default(),
        )
        .map_err(|e| invalid(&e))?;
        let mmproj = self
            .mmproj_path
            .to_str()
            .ok_or_else(|| ProviderError::InvalidRequest("mmproj 路径非 UTF-8".into()))?;
        let params = MtmdContextParams {
            n_threads: self.n_threads,
            ..MtmdContextParams::default()
        };
        let mtmd = MtmdContext::init_from_file(mmproj, &model, &params).map_err(|e| invalid(&e))?;
        if !mtmd.support_audio() {
            return Err(ProviderError::InvalidRequest(
                "Qwen3-ASR 模型不含音频编码器（mmproj 文件不匹配）".into(),
            ));
        }
        let loaded = Arc::new(LoadedAsr { model, mtmd });
        *guard = Some(Arc::clone(&loaded));
        Ok(loaded)
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

/// 阻塞转写：音频 chunk 评估进 KV cache 后贪心解码。
fn transcribe_blocking(loaded: &LoadedAsr, samples: &[f32]) -> Result<String, ProviderError> {
    let invalid = |e: &dyn std::fmt::Display| ProviderError::InvalidRequest(format!("{e}"));
    let model = &loaded.model;
    let mtmd = &loaded.mtmd;

    // prompt：用户消息 = 音频 marker + 转写指令（模板优先模型内置，缺失拼 ChatML）
    let marker = mtmd_default_marker();
    let user_content = format!("{marker}请转写这段音频。");
    let prompt = if let Ok(template) = model.chat_template(None) {
        let chat = vec![
            LlamaChatMessage::new("user".into(), user_content.clone()).map_err(|e| invalid(&e))?,
        ];
        model
            .apply_chat_template(&template, &chat, true)
            .map_err(|e| invalid(&e))?
    } else {
        format!("<|im_start|>user\n{user_content}<|im_end|>\n<|im_start|>assistant\n")
    };

    let bitmap = MtmdBitmap::from_audio_data(samples)
        .map_err(|e| ProviderError::InvalidRequest(format!("音频 embedding 构造失败: {e}")))?;
    let chunks = mtmd
        .tokenize(
            MtmdInputText {
                text: prompt,
                add_special: true,
                parse_special: true,
            },
            &[&bitmap],
        )
        .map_err(|e| invalid(&e))?;

    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(N_CTX))
        .with_n_batch(N_CTX)
        .with_n_threads(0) // 0 = llama.cpp 自动
        ;
    let mut ctx = model
        .new_context(llama_backend(), ctx_params)
        .map_err(|e| invalid(&e))?;

    let first_pos = chunks
        .eval_chunks(mtmd, &ctx, 0, 0, N_CTX as i32, true)
        .map_err(|e| invalid(&e))?;

    // 贪心解码到 EOG（转写任务无需采样随机性）
    let mut sampler = LlamaSampler::greedy();
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut out = String::new();
    let mut batch = LlamaBatch::new(1, 1);
    // 首个 token 采样自 eval_chunks 的最后 logits（idx -1 = 最后一个）
    let mut idx = -1;
    for n_past in first_pos..first_pos + MAX_NEW_TOKENS as i32 {
        let token = sampler.sample(&ctx, idx);
        if model.is_eog_token(token) {
            break;
        }
        let piece = model
            .token_to_piece(token, &mut decoder, false, None)
            .map_err(|e| invalid(&e))?;
        out.push_str(&piece);
        batch.clear();
        batch
            .add(token, n_past, &[0], true)
            .map_err(|e| invalid(&e))?;
        ctx.decode(&mut batch).map_err(|e| invalid(&e))?;
        idx = batch.n_tokens() - 1;
    }
    Ok(out)
}

#[async_trait::async_trait]
impl SttProvider for QwenAsrStt {
    async fn transcribe(
        &self,
        audio: AudioInput,
        _opts: SttOptions,
    ) -> Result<Transcript, ProviderError> {
        let samples = wav_to_samples(&audio.wav_16k_mono)?;
        let loaded = self.ensure_loaded()?;
        // 推理是秒级 CPU/GPU 阻塞调用：挪到 blocking 线程池，别占 async 线程
        let text = tokio::task::spawn_blocking(move || transcribe_blocking(&loaded, &samples))
            .await
            .map_err(|e| ProviderError::InvalidRequest(format!("转写线程异常: {e}")))??;
        Ok(Transcript {
            text: text.trim().to_string(),
            detected_language: None, // Qwen3-ASR 不单独回报语种
        })
    }

    fn capabilities(&self) -> SttCapabilities {
        SttCapabilities {
            // 保守上限强制 VAD 切片，规避 llama.cpp 长音频 bug（ADR-22）
            max_bytes: Some(MAX_AUDIO_BYTES),
            supports_prompt: false,
            supports_language: false,
        }
    }
}

// CString 仅用于 MtmdContextParams 的 media_marker 默认值路径；显式引用避免未用告警
#[allow(dead_code)]
fn _marker_cstring() -> CString {
    CString::new(mtmd_default_marker()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_invalid_request_not_panic() {
        let stt = QwenAsrStt::new(
            PathBuf::from("/nonexistent/model.gguf"),
            PathBuf::from("/nonexistent/mmproj.gguf"),
            4,
        );
        let err = stt.ensure_loaded().unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
        assert!(err.to_string().contains("模型未下载"));
    }

    #[test]
    fn capabilities_force_chunking_for_long_audio() {
        let stt = QwenAsrStt::new(PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b"), 4);
        let caps = stt.capabilities();
        assert_eq!(caps.max_bytes, Some(MAX_AUDIO_BYTES));
    }

    #[test]
    fn wav_parse_error_classified() {
        let err = wav_to_samples(b"not a wav").unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
    }
}
