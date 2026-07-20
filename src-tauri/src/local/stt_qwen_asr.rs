//! 本地 STT Provider · Qwen3-ASR 标准/性能档（03 §2.3 / ADR-22）。
//!
//! llama.cpp mtmd（多模态，experimental）跑 Qwen3-ASR GGUF：16k mono WAV →
//! f32 采样 → mtmd 音频 bitmap → encode → 自回归解码转写。除主模型 GGUF 外
//! 还需 mmproj（音频编码器投影）文件——两者都由模型下载管理器落盘。
//!
//! llama.cpp 长音频有已知 bug（03 §2.3 / ADR-22 工程注意）：`capabilities()`
//! 报保守的 max_bytes，上层 `transcribe_auto_chunk` 会在 VAD 静音处切片规避。

use crate::local::llm_llama::{
    InferenceModelCache, ModeLoaded, ModelLoadMode, RuntimeAttemptError, context_params_for_mode,
    execute_runtime_with_cpu_fallback, llama_backend, load_cpu_model, load_model_with_cpu_fallback,
    validate_gguf_header,
};
use crate::providers::ProviderError;
use crate::providers::stt::{
    AudioInput, NativeSttJobGate, SttCapabilities, SttOptions, SttProvider, Transcript,
    transcript_from_provider_text,
};
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
use llama_cpp_2::mtmd::{
    MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText, mtmd_default_marker,
};
use llama_cpp_2::sampling::LlamaSampler;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    state: Arc<InferenceModelCache<LoadedAsr>>,
    native_jobs: NativeSttJobGate,
    model_path: PathBuf,
    mmproj_path: PathBuf,
    n_threads: i32,
}

impl QwenAsrStt {
    /// `model_path` = 主模型 GGUF，`mmproj_path` = 音频编码器投影 GGUF
    /// （均在 `{data_dir}/models/{model_id}/` 下，构造函数注入）。
    pub fn new(model_path: PathBuf, mmproj_path: PathBuf, n_threads: i32) -> Self {
        Self {
            state: Arc::new(InferenceModelCache::new()),
            native_jobs: NativeSttJobGate::new(),
            model_path,
            mmproj_path,
            n_threads,
        }
    }

    /// 释放常驻内存（运行时策略切换用）。进行中的转写持有自己的 Arc，不受影响。
    pub fn unload(&self) {
        self.state.unload();
    }
}

fn validate_asr_files(model_path: &Path, mmproj_path: &Path) -> Result<(), ProviderError> {
    if !model_path.exists() || !mmproj_path.exists() {
        return Err(ProviderError::InvalidRequest(
            "模型未下载：请在设置-模型服务中下载 Qwen3-ASR".into(),
        ));
    }
    validate_gguf_header(mmproj_path, "Qwen3-ASR mmproj 加载失败")
}

fn mtmd_params_for_mode(n_threads: i32, mode: ModelLoadMode) -> MtmdContextParams {
    MtmdContextParams {
        use_gpu: mode.uses_gpu(),
        n_threads,
        ..MtmdContextParams::default()
    }
}

fn initialize_mtmd(
    model: &LlamaModel,
    mmproj_path: &Path,
    n_threads: i32,
    mode: ModelLoadMode,
) -> std::result::Result<MtmdContext, RuntimeAttemptError<ProviderError>> {
    let mmproj = mmproj_path.to_str().ok_or_else(|| {
        RuntimeAttemptError::input(ProviderError::InvalidRequest(
            "Qwen3-ASR mmproj 路径编码无效".into(),
        ))
    })?;
    let params = mtmd_params_for_mode(n_threads, mode);
    let mtmd = MtmdContext::init_from_file(mmproj, model, &params).map_err(|_| {
        RuntimeAttemptError::runtime(
            ProviderError::InvalidRequest("Qwen3-ASR 音频上下文初始化失败".into()),
            false,
        )
    })?;
    if !mtmd.support_audio() {
        return Err(RuntimeAttemptError::input(ProviderError::InvalidRequest(
            "Qwen3-ASR 模型不含音频编码器（mmproj 文件不匹配）".into(),
        )));
    }
    Ok(mtmd)
}

fn load_asr_with_cpu_fallback(
    model_path: &Path,
    mmproj_path: &Path,
    n_threads: i32,
) -> Result<ModeLoaded<LoadedAsr>, ProviderError> {
    validate_asr_files(model_path, mmproj_path)?;
    let initial_model = load_model_with_cpu_fallback(model_path, "Qwen3-ASR 模型加载失败")?;
    let execution = execute_runtime_with_cpu_fallback(
        initial_model,
        ModeLoaded::mode,
        |loaded| initialize_mtmd(loaded.value(), mmproj_path, n_threads, loaded.mode()),
        |_| {
            tracing::warn!(
                component = "local_asr",
                from = "gpu",
                to = "cpu",
                "local ASR runtime initialization failed; retrying once"
            );
            load_cpu_model(model_path, "Qwen3-ASR CPU 模型加载失败")
                .map(|model| ModeLoaded::new(model, ModelLoadMode::CpuOnly))
        },
    );
    let mtmd = execution.result?;
    let (model, mode) = execution
        .final_model
        .ok_or_else(|| ProviderError::InvalidRequest("Qwen3-ASR 模型初始化状态不一致".into()))?
        .into_parts();
    Ok(ModeLoaded::new(LoadedAsr { model, mtmd }, mode))
}

fn load_asr_cpu(
    model_path: &Path,
    mmproj_path: &Path,
    n_threads: i32,
) -> Result<LoadedAsr, ProviderError> {
    validate_asr_files(model_path, mmproj_path)?;
    let model = load_cpu_model(model_path, "Qwen3-ASR CPU 模型加载失败")?;
    let mtmd = initialize_mtmd(&model, mmproj_path, n_threads, ModelLoadMode::CpuOnly)
        .map_err(RuntimeAttemptError::into_error)?;
    Ok(LoadedAsr { model, mtmd })
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
fn transcribe_blocking(
    loaded: &LoadedAsr,
    mode: ModelLoadMode,
    samples: &[f32],
    dictionary_prompt: Option<&str>,
) -> std::result::Result<String, RuntimeAttemptError<ProviderError>> {
    let input_error = |message: &'static str| {
        RuntimeAttemptError::input(ProviderError::InvalidRequest(message.into()))
    };
    let runtime_error = |message: &'static str| {
        RuntimeAttemptError::runtime(ProviderError::InvalidRequest(message.into()), false)
    };
    let model = &loaded.model;
    let mtmd = &loaded.mtmd;

    // prompt：用户消息 = 音频 marker + 转写指令（模板优先模型内置，缺失拼 ChatML）
    let marker = mtmd_default_marker();
    let user_content = match dictionary_prompt {
        Some(prompt) if !prompt.trim().is_empty() => {
            format!(
                "{marker}请转写这段音频。以下是用户词典，请在听到相近发音时优先使用这些标准写法，不要输出词典中没有被说出的词：\n{}",
                prompt.trim()
            )
        }
        _ => format!("{marker}请转写这段音频。"),
    };
    let prompt = if let Ok(template) = model.chat_template(None) {
        let chat = vec![
            LlamaChatMessage::new("user".into(), user_content.clone())
                .map_err(|_| input_error("Qwen3-ASR prompt 消息格式无效"))?,
        ];
        model
            .apply_chat_template(&template, &chat, true)
            .map_err(|_| input_error("Qwen3-ASR prompt 模板渲染失败"))?
    } else {
        format!("<|im_start|>user\n{user_content}<|im_end|>\n<|im_start|>assistant\n")
    };

    let bitmap = MtmdBitmap::from_audio_data(samples)
        .map_err(|_| input_error("Qwen3-ASR 音频 embedding 构造失败"))?;
    let chunks = mtmd
        .tokenize(
            MtmdInputText {
                text: prompt,
                add_special: true,
                parse_special: true,
            },
            &[&bitmap],
        )
        .map_err(|_| input_error("Qwen3-ASR 多模态输入 tokenize 失败"))?;

    let ctx_params = context_params_for_mode(mode, N_CTX, N_CTX).with_n_threads(0); // 0 = llama.cpp 自动
    let mut ctx = model
        .new_context(llama_backend(), ctx_params)
        .map_err(|_| runtime_error("Qwen3-ASR 运行时上下文初始化失败"))?;

    let first_pos = chunks
        .eval_chunks(mtmd, &ctx, 0, 0, N_CTX as i32, true)
        .map_err(|_| runtime_error("Qwen3-ASR 音频 decode 失败"))?;

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
            .map_err(|_| input_error("Qwen3-ASR token 解码失败"))?;
        out.push_str(&piece);
        batch.clear();
        batch
            .add(token, n_past, &[0], true)
            .map_err(|_| input_error("Qwen3-ASR token batch 构造失败"))?;
        ctx.decode(&mut batch)
            .map_err(|_| runtime_error("Qwen3-ASR token decode 失败"))?;
        idx = batch.n_tokens() - 1;
    }
    Ok(out)
}

#[async_trait::async_trait]
impl SttProvider for QwenAsrStt {
    async fn transcribe(
        &self,
        audio: AudioInput,
        opts: SttOptions,
    ) -> Result<Transcript, ProviderError> {
        let state = Arc::clone(&self.state);
        let model_path = self.model_path.clone();
        let mmproj_path = self.mmproj_path.clone();
        let n_threads = self.n_threads;
        let dictionary_prompt = opts.prompt;
        let text = self
            .native_jobs
            .run("Qwen3-ASR 转写任务", move || {
                let samples = wav_to_samples(&audio.wav_16k_mono)?;
                let lease = state.acquire();
                let initial_model = lease.get_or_try_init(|| {
                    load_asr_with_cpu_fallback(&model_path, &mmproj_path, n_threads)
                })?;
                execute_runtime_with_cpu_fallback(
                    initial_model,
                    |loaded| loaded.mode(),
                    |loaded| {
                        transcribe_blocking(
                            loaded.value(),
                            loaded.mode(),
                            &samples,
                            dictionary_prompt.as_deref(),
                        )
                    },
                    |failed| {
                        tracing::warn!(
                            component = "local_asr",
                            from = "gpu",
                            to = "cpu",
                            "local ASR decode failed; retrying the full transcription once"
                        );
                        lease.replace_gpu_with_cpu(failed, || {
                            load_asr_cpu(&model_path, &mmproj_path, n_threads)
                        })
                    },
                )
                .result
            })
            .await?;
        Ok(transcript_from_provider_text(text, None))
    }

    fn capabilities(&self) -> SttCapabilities {
        SttCapabilities {
            // 保守上限强制 VAD 切片，规避 llama.cpp 长音频 bug（ADR-22）
            max_bytes: Some(MAX_AUDIO_BYTES),
            supports_prompt: true,
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
    use std::io::Write;

    #[test]
    fn missing_model_is_invalid_request_not_panic() {
        let stt = QwenAsrStt::new(
            PathBuf::from("/nonexistent/model.gguf"),
            PathBuf::from("/nonexistent/mmproj.gguf"),
            4,
        );
        let lease = stt.state.acquire();
        let err = lease
            .get_or_try_init(|| {
                load_asr_with_cpu_fallback(&stt.model_path, &stt.mmproj_path, stt.n_threads)
            })
            .unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
        assert!(err.to_string().contains("模型未下载"));
    }

    #[test]
    fn cpu_mtmd_params_disable_gpu() {
        assert!(mtmd_params_for_mode(4, ModelLoadMode::GpuOffload).use_gpu);
        assert!(!mtmd_params_for_mode(4, ModelLoadMode::CpuOnly).use_gpu);
    }

    #[test]
    fn capabilities_force_chunking_for_long_audio() {
        let stt = QwenAsrStt::new(PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b"), 4);
        let caps = stt.capabilities();
        assert_eq!(caps.max_bytes, Some(MAX_AUDIO_BYTES));
        assert!(caps.supports_prompt);
    }

    #[test]
    fn wav_parse_error_classified() {
        let err = wav_to_samples(b"not a wav").unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
    }

    #[test]
    fn invalid_mmproj_is_rejected_without_exposing_its_path() {
        let mut model = tempfile::NamedTempFile::new().unwrap();
        model.write_all(b"GGUF").unwrap();
        let mut mmproj = tempfile::NamedTempFile::new().unwrap();
        mmproj.write_all(b"not a gguf").unwrap();

        let error = validate_asr_files(model.path(), mmproj.path()).unwrap_err();

        assert!(error.to_string().contains("不是有效的 GGUF"));
        assert!(
            !error
                .to_string()
                .contains(&mmproj.path().display().to_string())
        );
    }

    #[test]
    fn non_streaming_runtime_failure_replays_entire_attempt_once_on_cpu() {
        let mut attempts = Vec::new();
        let execution = execute_runtime_with_cpu_fallback(
            ModeLoaded::new("gpu ASR", ModelLoadMode::GpuOffload),
            ModeLoaded::mode,
            |model| {
                attempts.push(model.mode());
                match model.mode() {
                    ModelLoadMode::GpuOffload => {
                        Err(RuntimeAttemptError::runtime("GPU eval failed", false))
                    }
                    ModelLoadMode::CpuOnly => Ok("transcript"),
                }
            },
            |_| Ok(ModeLoaded::new("cpu ASR", ModelLoadMode::CpuOnly)),
        );

        assert_eq!(execution.result, Ok("transcript"));
        assert_eq!(
            execution.final_model.as_ref().unwrap().mode(),
            ModelLoadMode::CpuOnly
        );
        assert_eq!(
            attempts,
            vec![ModelLoadMode::GpuOffload, ModelLoadMode::CpuOnly]
        );
    }

    #[test]
    fn non_streaming_input_error_does_not_reload_cpu_model() {
        let mut fallback_called = false;
        let execution = execute_runtime_with_cpu_fallback(
            ModeLoaded::new("gpu ASR", ModelLoadMode::GpuOffload),
            ModeLoaded::mode,
            |_| -> std::result::Result<(), RuntimeAttemptError<&str>> {
                Err(RuntimeAttemptError::input("invalid audio"))
            },
            |_| {
                fallback_called = true;
                Ok(ModeLoaded::new("cpu ASR", ModelLoadMode::CpuOnly))
            },
        );

        assert_eq!(execution.result.unwrap_err(), "invalid audio");
        assert!(!fallback_called);
    }
}
