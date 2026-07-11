//! 本地 LLM Provider · llama.cpp + Qwen3.5 GGUF（03 §3.3 / ADR-20/22）。
//!
//! 实现同一 `LlmProvider` trait（流式 delta 与云端一致）：推理在专属线程逐
//! token 生成，经 tokio mpsc channel 转成 BoxStream。上下文 4K（整理/翻译都是
//! 短输入）；chat 模板优先用模型内置（GGUF metadata），缺失时手拼 Qwen ChatML。
//! 错误分类只剩 InvalidRequest / 模型未下载（03 §3.3）。

use crate::providers::ProviderError;
use crate::providers::llm::{LlmCapabilities, LlmDelta, LlmProvider, LlmRequest, ThinkingFilter};
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use llama_cpp_2::LlamaModelLoadError;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use std::cell::Cell;
use std::io::Read;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// 上下文窗口（03 §3.3：整理/翻译短输入，4K 足够）。
const N_CTX: u32 = 4096;
/// 未显式指定 max_tokens 时的生成上限（防跑飞；整理/翻译输出远小于此）。
const DEFAULT_MAX_TOKENS: u32 = 1024;
const THINK_DIRECTIVE: &str = "/think";
const NO_THINK_DIRECTIVE: &str = "/no_think";

static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

/// LlamaBackend 全局单例（llama-cpp-2 要求 init 只能调用一次）。
/// llm_llama 与 stt_qwen_asr 共用。
pub(crate) fn llama_backend() -> &'static LlamaBackend {
    BACKEND.get_or_init(|| {
        let mut backend = LlamaBackend::init().expect("LlamaBackend::init 只在 OnceLock 内调一次");
        // llama.cpp 默认把加载日志刷到 stderr——静默（日志纪律：只记长度与耗时）
        backend.void_logs();
        backend
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelLoadMode {
    GpuOffload,
    CpuOnly,
}

impl ModelLoadMode {
    pub(crate) fn uses_gpu(self) -> bool {
        self == Self::GpuOffload
    }
}

#[derive(Debug)]
pub(crate) struct ModeLoaded<T> {
    value: T,
    mode: ModelLoadMode,
}

impl<T> ModeLoaded<T> {
    pub(crate) fn new(value: T, mode: ModelLoadMode) -> Self {
        Self { value, mode }
    }

    pub(crate) fn mode(&self) -> ModelLoadMode {
        self.mode
    }

    pub(crate) fn value(&self) -> &T {
        &self.value
    }

    pub(crate) fn into_parts(self) -> (T, ModelLoadMode) {
        (self.value, self.mode)
    }
}

#[derive(Debug)]
struct ModelCacheState<T> {
    current: Option<Arc<ModeLoaded<T>>>,
    revision: u64,
}

#[derive(Debug)]
pub(crate) struct InferenceModelCache<T> {
    state: Mutex<ModelCacheState<T>>,
    inference: Mutex<()>,
}

#[derive(Debug)]
pub(crate) struct CachedModel<'lease, T> {
    loaded: Arc<ModeLoaded<T>>,
    revision: u64,
    checkout: &'lease Cell<bool>,
}

pub(crate) struct InferenceLease<'cache, T> {
    cache: &'cache InferenceModelCache<T>,
    _guard: MutexGuard<'cache, ()>,
    checked_out: Cell<bool>,
}

impl<T> CachedModel<'_, T> {
    pub(crate) fn mode(&self) -> ModelLoadMode {
        self.loaded.mode()
    }

    pub(crate) fn value(&self) -> &T {
        self.loaded.value()
    }
}

impl<T> Drop for CachedModel<'_, T> {
    fn drop(&mut self) {
        debug_assert!(self.checkout.replace(false));
    }
}

impl<T> InferenceModelCache<T> {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(ModelCacheState {
                current: None,
                revision: 0,
            }),
            inference: Mutex::new(()),
        }
    }

    pub(crate) fn acquire(&self) -> InferenceLease<'_, T> {
        InferenceLease {
            cache: self,
            _guard: self.inference.lock().unwrap(),
            checked_out: Cell::new(false),
        }
    }

    pub(crate) fn unload(&self) {
        let mut state = self.state.lock().unwrap();
        state.current = None;
        state.revision = state.revision.wrapping_add(1);
    }

    #[cfg(test)]
    fn cached_mode(&self) -> Option<ModelLoadMode> {
        self.state
            .lock()
            .unwrap()
            .current
            .as_ref()
            .map(|loaded| loaded.mode())
    }
}

impl<T> InferenceLease<'_, T> {
    fn checkout(&self, loaded: Arc<ModeLoaded<T>>, revision: u64) -> CachedModel<'_, T> {
        assert!(
            !self.checked_out.replace(true),
            "an inference lease can hold only one live model handle"
        );
        CachedModel {
            loaded,
            revision,
            checkout: &self.checked_out,
        }
    }

    pub(crate) fn get_or_try_init<E>(
        &self,
        load: impl FnOnce() -> std::result::Result<ModeLoaded<T>, E>,
    ) -> std::result::Result<CachedModel<'_, T>, E> {
        let start_revision = {
            let state = self.cache.state.lock().unwrap();
            if let Some(loaded) = state.current.as_ref() {
                return Ok(self.checkout(Arc::clone(loaded), state.revision));
            }
            state.revision
        };

        let loaded = Arc::new(load()?);
        let mut state = self.cache.state.lock().unwrap();
        if state.revision == start_revision && state.current.is_none() {
            state.revision = state.revision.wrapping_add(1);
            let revision = state.revision;
            state.current = Some(Arc::clone(&loaded));
            return Ok(self.checkout(loaded, revision));
        }
        Ok(self.checkout(loaded, state.revision))
    }

    /// Replace only the exact failed GPU cache generation. If another request already installed a
    /// CPU model, reuse it; if unload/reload changed the generation, return a detached CPU model
    /// for this request without repopulating or clobbering the cache.
    pub(crate) fn replace_gpu_with_cpu<E>(
        &self,
        failed: CachedModel<'_, T>,
        load_cpu: impl FnOnce() -> std::result::Result<T, E>,
    ) -> std::result::Result<CachedModel<'_, T>, E> {
        let mut state = self.cache.state.lock().unwrap();
        if let Some(current) = state.current.as_ref()
            && current.mode() == ModelLoadMode::CpuOnly
        {
            let loaded = Arc::clone(current);
            let revision = state.revision;
            drop(state);
            drop(failed);
            return Ok(self.checkout(loaded, revision));
        }

        let replace_current = failed.revision == state.revision
            && state
                .current
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &failed.loaded));
        if replace_current {
            let cached_gpu = state.current.take();
            state.revision = state.revision.wrapping_add(1);
            let replacement_revision = state.revision;
            drop(state);
            drop(cached_gpu);
            drop(failed);
            let loaded = Arc::new(ModeLoaded::new(load_cpu()?, ModelLoadMode::CpuOnly));
            let mut state = self.cache.state.lock().unwrap();
            if state.revision == replacement_revision && state.current.is_none() {
                state.current = Some(Arc::clone(&loaded));
                return Ok(self.checkout(loaded, replacement_revision));
            }
            return Ok(self.checkout(loaded, state.revision));
        }

        let revision = state.revision;
        drop(state);
        drop(failed);
        let loaded = Arc::new(ModeLoaded::new(load_cpu()?, ModelLoadMode::CpuOnly));
        Ok(self.checkout(loaded, revision))
    }

    pub(crate) fn clear_if_current(&self, used: &CachedModel<'_, T>) {
        let mut state = self.cache.state.lock().unwrap();
        let is_current = used.revision == state.revision
            && state
                .current
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &used.loaded));
        if is_current {
            state.current = None;
            state.revision = state.revision.wrapping_add(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeFailureClass {
    Input,
    Runtime,
}

#[derive(Debug)]
pub(crate) struct RuntimeAttemptError<E> {
    error: E,
    class: RuntimeFailureClass,
    visible_output_emitted: bool,
}

impl<E> RuntimeAttemptError<E> {
    pub(crate) fn input(error: E) -> Self {
        Self {
            error,
            class: RuntimeFailureClass::Input,
            visible_output_emitted: false,
        }
    }

    pub(crate) fn runtime(error: E, visible_output_emitted: bool) -> Self {
        Self {
            error,
            class: RuntimeFailureClass::Runtime,
            visible_output_emitted,
        }
    }

    pub(crate) fn into_error(self) -> E {
        self.error
    }
}

fn should_retry_runtime_on_cpu<E>(
    mode: ModelLoadMode,
    cpu_retry_used: bool,
    failure: &RuntimeAttemptError<E>,
) -> bool {
    mode.uses_gpu()
        && !cpu_retry_used
        && failure.class == RuntimeFailureClass::Runtime
        && !failure.visible_output_emitted
}

pub(crate) struct RuntimeExecution<M, T, E> {
    pub(crate) result: std::result::Result<T, E>,
    pub(crate) final_model: Option<M>,
}

pub(crate) fn execute_runtime_with_cpu_fallback<M, T, E>(
    initial_model: M,
    mode_of: impl Fn(&M) -> ModelLoadMode,
    mut run: impl FnMut(&M) -> std::result::Result<T, RuntimeAttemptError<E>>,
    mut cpu_fallback: impl FnMut(M) -> std::result::Result<M, E>,
) -> RuntimeExecution<M, T, E> {
    let current = initial_model;
    match run(&current) {
        Ok(value) => RuntimeExecution {
            result: Ok(value),
            final_model: Some(current),
        },
        Err(failure) if should_retry_runtime_on_cpu(mode_of(&current), false, &failure) => {
            match cpu_fallback(current) {
                Ok(cpu_model) => {
                    let result = run(&cpu_model).map_err(|failure| failure.error);
                    RuntimeExecution {
                        result,
                        final_model: Some(cpu_model),
                    }
                }
                Err(error) => RuntimeExecution {
                    result: Err(error),
                    final_model: None,
                },
            }
        }
        Err(failure) => RuntimeExecution {
            result: Err(failure.error),
            final_model: Some(current),
        },
    }
}

fn params_for_load(mode: ModelLoadMode) -> LlamaModelParams {
    match mode {
        ModelLoadMode::GpuOffload => LlamaModelParams::default(),
        ModelLoadMode::CpuOnly => LlamaModelParams::default()
            .with_n_gpu_layers(0)
            .with_devices(&[])
            .expect("an empty llama device list is always valid"),
    }
}

pub(crate) fn context_params_for_mode(
    mode: ModelLoadMode,
    n_ctx: u32,
    n_batch: u32,
) -> LlamaContextParams {
    let params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(n_ctx))
        .with_n_batch(n_batch);
    match mode {
        ModelLoadMode::GpuOffload => params,
        ModelLoadMode::CpuOnly => params.with_offload_kqv(false).with_op_offload(false),
    }
}

fn load_with_cpu_fallback<T, E>(
    gpu_offload_available: bool,
    mut load: impl FnMut(ModelLoadMode) -> std::result::Result<T, E>,
    retryable_on_cpu: impl Fn(&E) -> bool,
) -> std::result::Result<ModeLoaded<T>, E> {
    let initial_mode = if gpu_offload_available {
        ModelLoadMode::GpuOffload
    } else {
        ModelLoadMode::CpuOnly
    };
    match load(initial_mode) {
        Ok(value) => Ok(ModeLoaded::new(value, initial_mode)),
        Err(error) if initial_mode.uses_gpu() && retryable_on_cpu(&error) => {
            load(ModelLoadMode::CpuOnly).map(|value| ModeLoaded::new(value, ModelLoadMode::CpuOnly))
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn validate_gguf_header(path: &Path, label: &str) -> Result<(), ProviderError> {
    let mut file = std::fs::File::open(path)
        .map_err(|_| ProviderError::InvalidRequest(format!("{label}: 模型文件无法读取")))?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)
        .map_err(|_| ProviderError::InvalidRequest(format!("{label}: 模型文件不完整")))?;
    if magic != *b"GGUF" {
        return Err(ProviderError::InvalidRequest(format!(
            "{label}: 模型文件不是有效的 GGUF"
        )));
    }
    Ok(())
}

fn sanitized_model_load_error(label: &str, error: LlamaModelLoadError) -> ProviderError {
    let detail = match error {
        LlamaModelLoadError::NullError(_) => "模型路径包含无效字符",
        LlamaModelLoadError::PathToStrError(_) => "模型路径编码无效",
        LlamaModelLoadError::NullResult => "模型文件无效或可用内存不足",
    };
    ProviderError::InvalidRequest(format!("{label}: {detail}"))
}

pub(crate) fn load_cpu_model(path: &Path, label: &str) -> Result<LlamaModel, ProviderError> {
    load_model_in_mode_impl(path, label, ModelLoadMode::CpuOnly)
}

fn load_model_in_mode_impl(
    path: &Path,
    label: &str,
    mode: ModelLoadMode,
) -> Result<LlamaModel, ProviderError> {
    validate_gguf_header(path, label)?;
    let backend = llama_backend();
    let params = params_for_load(mode);
    LlamaModel::load_from_file(backend, path, &params)
        .map_err(|error| sanitized_model_load_error(label, error))
}

pub(crate) fn load_model_with_cpu_fallback(
    path: &Path,
    label: &str,
) -> Result<ModeLoaded<LlamaModel>, ProviderError> {
    validate_gguf_header(path, label)?;
    let backend = llama_backend();
    load_with_cpu_fallback(
        backend.supports_gpu_offload(),
        |mode| {
            let params = params_for_load(mode);
            LlamaModel::load_from_file(backend, path, &params)
        },
        |error| matches!(error, LlamaModelLoadError::NullResult),
    )
    .map_err(|error| sanitized_model_load_error(label, error))
}

/// 运行时加载策略（03 §3.3：常驻内存 / 用完即卸，设置可选）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPolicy {
    /// 常驻内存：首次加载后保留（冷加载 1–3s 只付一次）。
    Resident,
    /// 用完即卸：每次 complete 结束后释放（省内存，每次付冷加载）。
    UnloadAfterUse,
}

pub struct LlamaLlm {
    /// 惰性加载；缓存代际使并发 fallback/unload 只替换或清理自己的条目。
    model: Arc<InferenceModelCache<LlamaModel>>,
    model_path: PathBuf,
    policy: LoadPolicy,
    enable_thinking: bool,
}

impl LlamaLlm {
    /// `model_path` = `{data_dir}/models/{model_id}/qwen3.5-*.gguf`（构造函数注入）。
    pub fn new(model_path: PathBuf, policy: LoadPolicy) -> Self {
        Self {
            model: Arc::new(InferenceModelCache::new()),
            model_path,
            policy,
            enable_thinking: false,
        }
    }

    pub fn with_thinking(mut self, enable_thinking: bool) -> Self {
        self.enable_thinking = enable_thinking;
        self
    }

    /// 释放常驻内存（运行时策略切换/内存压力时用）。
    /// 正在生成的会话持有自己的 Arc 克隆，不受影响，结束后内存才真正归还。
    pub fn unload(&self) {
        self.model.unload();
    }

    /// 模型文件缺失 → InvalidRequest「模型未下载」（03 §3.3，不 panic）。
    fn check_model_file(&self) -> Result<(), ProviderError> {
        if !self.model_path.exists() {
            return Err(ProviderError::InvalidRequest(
                "模型未下载：请在设置-模型服务中下载本地 LLM 模型".into(),
            ));
        }
        Ok(())
    }

    /// 加载（或复用）模型；持锁只覆盖加载本身，不覆盖生成。
    fn ensure_loaded<'lease>(
        lease: &'lease InferenceLease<'_, LlamaModel>,
        path: &Path,
    ) -> Result<CachedModel<'lease, LlamaModel>, ProviderError> {
        lease.get_or_try_init(|| load_model_with_cpu_fallback(path, "本地 LLM 模型加载失败"))
    }
}

fn thinking_directive(enable_thinking: bool) -> &'static str {
    if enable_thinking {
        THINK_DIRECTIVE
    } else {
        NO_THINK_DIRECTIVE
    }
}

fn append_thinking_directive(content: &str, enable_thinking: bool) -> String {
    let sep = if content.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{content}{sep}{}", thinking_directive(enable_thinking))
}

#[derive(Debug, Default)]
struct VisibleOutputTracker {
    filter: ThinkingFilter,
    emitted: bool,
}

impl VisibleOutputTracker {
    fn push(&mut self, piece: &str) -> Option<String> {
        let text = self.filter.push(piece);
        self.record(text)
    }

    fn finish(&mut self) -> Option<String> {
        let text = self.filter.finish();
        self.record(text)
    }

    fn record(&mut self, text: String) -> Option<String> {
        if text.is_empty() {
            None
        } else {
            self.emitted = true;
            Some(text)
        }
    }

    fn has_emitted(&self) -> bool {
        self.emitted
    }
}

/// 组装 prompt：优先模型内置 chat 模板（GGUF metadata），缺失时手拼 Qwen ChatML。
fn build_prompt(
    model: &LlamaModel,
    req: &LlmRequest,
    enable_thinking: bool,
) -> Result<String, ProviderError> {
    let last_user = req.messages.iter().rposition(|m| m.role == "user");
    if let Ok(template) = model.chat_template(None) {
        let mut chat = Vec::with_capacity(req.messages.len() + 1);
        if !req.system.is_empty() {
            chat.push(
                LlamaChatMessage::new("system".into(), req.system.clone())
                    .map_err(|_| ProviderError::InvalidRequest("本地 LLM 消息格式无效".into()))?,
            );
        }
        for (idx, m) in req.messages.iter().enumerate() {
            let content = if Some(idx) == last_user {
                append_thinking_directive(&m.content, enable_thinking)
            } else {
                m.content.clone()
            };
            chat.push(
                LlamaChatMessage::new(m.role.clone(), content)
                    .map_err(|_| ProviderError::InvalidRequest("本地 LLM 消息格式无效".into()))?,
            );
        }
        return model
            .apply_chat_template(&template, &chat, true)
            .map_err(|_| ProviderError::InvalidRequest("本地 LLM prompt 模板渲染失败".into()));
    }
    // 兜底：Qwen ChatML 格式
    let mut prompt = String::new();
    if !req.system.is_empty() {
        prompt.push_str(&format!("<|im_start|>system\n{}<|im_end|>\n", req.system));
    }
    for (idx, m) in req.messages.iter().enumerate() {
        let content = if Some(idx) == last_user {
            append_thinking_directive(&m.content, enable_thinking)
        } else {
            m.content.clone()
        };
        prompt.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", m.role, content));
    }
    prompt.push_str("<|im_start|>assistant\n");
    Ok(prompt)
}

/// 专属线程内的阻塞生成循环：逐 token 发 delta 到 channel。
fn generate_blocking(
    model: &LlamaModel,
    mode: ModelLoadMode,
    req: &LlmRequest,
    enable_thinking: bool,
    tx: &tokio::sync::mpsc::UnboundedSender<Result<LlmDelta, ProviderError>>,
) -> std::result::Result<(), RuntimeAttemptError<ProviderError>> {
    let input_error = |message: &'static str| {
        RuntimeAttemptError::input(ProviderError::InvalidRequest(message.into()))
    };
    let runtime_error = |message: &'static str, visible_output_emitted: bool| {
        let message = if visible_output_emitted && mode.uses_gpu() {
            "本地 LLM GPU 推理在流式输出后失败；为避免重复文本，未自动回退 CPU"
        } else if visible_output_emitted {
            "本地 LLM CPU 推理在流式输出后失败；为避免重复文本，未重放输出"
        } else {
            message
        };
        RuntimeAttemptError::runtime(
            ProviderError::InvalidRequest(message.into()),
            visible_output_emitted,
        )
    };

    let prompt = build_prompt(model, req, enable_thinking).map_err(RuntimeAttemptError::input)?;
    // 模板已含特殊 token，不再加 BOS
    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|_| input_error("本地 LLM prompt tokenize 失败"))?;
    let max_new = req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
    if tokens.len() as u32 + max_new > N_CTX {
        return Err(RuntimeAttemptError::input(ProviderError::InvalidRequest(
            format!(
                "输入过长：{} tokens 超出本地模型 {N_CTX} 上下文窗口",
                tokens.len()
            ),
        )));
    }

    let ctx_params = context_params_for_mode(mode, N_CTX, N_CTX);
    let mut ctx = model
        .new_context(llama_backend(), ctx_params)
        .map_err(|_| runtime_error("本地 LLM 运行时上下文初始化失败", false))?;

    let mut batch = LlamaBatch::new(N_CTX as usize, 1);
    batch
        .add_sequence(&tokens, 0, false)
        .map_err(|_| input_error("本地 LLM prompt batch 构造失败"))?;
    ctx.decode(&mut batch)
        .map_err(|_| runtime_error("本地 LLM prompt decode 失败", false))?;

    // 采样：temperature ≤ 0 视作贪心
    let mut sampler = if req.temperature > 0.0 {
        LlamaSampler::chain_simple([
            LlamaSampler::temp(req.temperature),
            LlamaSampler::dist(1234),
        ])
    } else {
        LlamaSampler::greedy()
    };

    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut visible_output = VisibleOutputTracker::default();
    let first_pos = tokens.len() as i32;
    for n_cur in first_pos..first_pos + max_new as i32 {
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        if model.is_eog_token(token) {
            break;
        }
        let piece = model
            .token_to_piece(token, &mut decoder, false, None)
            .map_err(|_| input_error("本地 LLM token 解码失败"))?;
        if let Some(visible_text) = visible_output.push(&piece)
            && tx.send(Ok(LlmDelta { text: visible_text })).is_err()
        {
            return Ok(()); // 下游已丢弃 stream（会话取消）——停止生成
        }
        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|_| input_error("本地 LLM token batch 构造失败"))?;
        ctx.decode(&mut batch).map_err(|_| {
            runtime_error("本地 LLM token decode 失败", visible_output.has_emitted())
        })?;
    }
    if let Some(visible_text) = visible_output.finish() {
        let _ = tx.send(Ok(LlmDelta { text: visible_text }));
    }
    Ok(())
}

impl LlmProvider for LlamaLlm {
    fn complete(&self, req: LlmRequest) -> BoxStream<'static, Result<LlmDelta, ProviderError>> {
        if let Err(e) = self.check_model_file() {
            return futures_util::stream::once(async move { Err(e) }).boxed();
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let slot = Arc::clone(&self.model);
        let path = self.model_path.clone();
        let policy = self.policy;
        let enable_thinking = self.enable_thinking;
        // 推理是长阻塞调用：专属线程，逐 token 经 channel 回流成 BoxStream
        std::thread::spawn(move || {
            let lease = slot.acquire();
            let final_model = match Self::ensure_loaded(&lease, &path) {
                Ok(initial_model) => {
                    let execution = execute_runtime_with_cpu_fallback(
                        initial_model,
                        CachedModel::mode,
                        |loaded| {
                            generate_blocking(
                                loaded.value(),
                                loaded.mode(),
                                &req,
                                enable_thinking,
                                &tx,
                            )
                        },
                        |failed| {
                            tracing::warn!(
                                component = "local_llm",
                                from = "gpu",
                                to = "cpu",
                                "local inference runtime failed before visible output; retrying once"
                            );
                            lease.replace_gpu_with_cpu(failed, || {
                                load_cpu_model(&path, "本地 LLM CPU 模型加载失败")
                            })
                        },
                    );
                    if let Err(error) = execution.result {
                        let _ = tx.send(Err(error));
                    }
                    execution.final_model
                }
                Err(error) => {
                    let _ = tx.send(Err(error));
                    None
                }
            };
            if policy == LoadPolicy::UnloadAfterUse
                && let Some(final_model) = final_model.as_ref()
            {
                lease.clear_if_current(final_model);
            }
        });
        let stream = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        stream.boxed()
    }

    fn capabilities(&self) -> LlmCapabilities {
        LlmCapabilities { streaming: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    fn dummy_request() -> LlmRequest {
        LlmRequest {
            system: "你是文本整理助手".into(),
            messages: vec![crate::providers::llm::Msg {
                role: "user".into(),
                content: "测试".into(),
            }],
            temperature: 0.3,
            max_tokens: Some(64),
        }
    }

    #[tokio::test]
    async fn missing_model_yields_invalid_request_not_panic() {
        let llm = LlamaLlm::new(
            PathBuf::from("/nonexistent/qwen3.5.gguf"),
            LoadPolicy::Resident,
        );
        let mut stream = llm.complete(dummy_request());
        let first = stream.next().await.expect("应产出一条错误");
        let err = first.unwrap_err();
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
        assert!(err.to_string().contains("模型未下载"));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeLoadError {
        Gpu,
        InvalidModel,
        Cpu,
    }

    #[test]
    fn gpu_load_failure_retries_once_with_cpu_params() {
        let mut attempts = Vec::new();
        let loaded = load_with_cpu_fallback(
            true,
            |mode| {
                attempts.push(mode);
                match mode {
                    ModelLoadMode::GpuOffload => Err(FakeLoadError::Gpu),
                    ModelLoadMode::CpuOnly => Ok("cpu model"),
                }
            },
            |error| *error == FakeLoadError::Gpu,
        )
        .unwrap();

        assert_eq!(*loaded.value(), "cpu model");
        assert_eq!(loaded.mode(), ModelLoadMode::CpuOnly);
        assert_eq!(
            attempts,
            vec![ModelLoadMode::GpuOffload, ModelLoadMode::CpuOnly]
        );
        assert_eq!(
            params_for_load(ModelLoadMode::GpuOffload).n_gpu_layers(),
            -1
        );
        let cpu_model = params_for_load(ModelLoadMode::CpuOnly);
        assert_eq!(cpu_model.n_gpu_layers(), 0);
        assert!(cpu_model.devices().is_empty());
    }

    #[test]
    fn cpu_context_disables_all_runtime_gpu_offload() {
        let gpu = context_params_for_mode(ModelLoadMode::GpuOffload, N_CTX, N_CTX);
        assert!(gpu.offload_kqv());
        assert!(gpu.op_offload());

        let cpu = context_params_for_mode(ModelLoadMode::CpuOnly, N_CTX, N_CTX);
        assert!(!cpu.offload_kqv());
        assert!(!cpu.op_offload());
    }

    #[test]
    fn no_gpu_or_non_retryable_error_does_not_retry() {
        for (gpu_available, error) in [
            (false, FakeLoadError::Gpu),
            (true, FakeLoadError::InvalidModel),
        ] {
            let mut attempts = Vec::new();
            let result: std::result::Result<ModeLoaded<()>, FakeLoadError> = load_with_cpu_fallback(
                gpu_available,
                |mode| {
                    attempts.push(mode);
                    Err(error)
                },
                |candidate| *candidate == FakeLoadError::Gpu,
            );

            assert_eq!(result.unwrap_err(), error);
            let expected_mode = if gpu_available {
                ModelLoadMode::GpuOffload
            } else {
                ModelLoadMode::CpuOnly
            };
            assert_eq!(attempts, vec![expected_mode]);
        }
    }

    #[test]
    fn cpu_retry_error_is_the_final_error() {
        let result: std::result::Result<ModeLoaded<()>, FakeLoadError> = load_with_cpu_fallback(
            true,
            |mode| match mode {
                ModelLoadMode::GpuOffload => Err(FakeLoadError::Gpu),
                ModelLoadMode::CpuOnly => Err(FakeLoadError::Cpu),
            },
            |error| *error == FakeLoadError::Gpu,
        );

        assert_eq!(result.unwrap_err(), FakeLoadError::Cpu);
    }

    #[test]
    fn runtime_retry_decision_requires_gpu_runtime_failure_before_visible_output() {
        let retryable = RuntimeAttemptError::runtime("gpu decode", false);
        assert!(should_retry_runtime_on_cpu(
            ModelLoadMode::GpuOffload,
            false,
            &retryable
        ));
        assert!(!should_retry_runtime_on_cpu(
            ModelLoadMode::CpuOnly,
            false,
            &retryable
        ));
        assert!(!should_retry_runtime_on_cpu(
            ModelLoadMode::GpuOffload,
            true,
            &retryable
        ));

        let after_output = RuntimeAttemptError::runtime("gpu decode", true);
        assert!(!should_retry_runtime_on_cpu(
            ModelLoadMode::GpuOffload,
            false,
            &after_output
        ));
        let input = RuntimeAttemptError::input("bad prompt");
        assert!(!should_retry_runtime_on_cpu(
            ModelLoadMode::GpuOffload,
            false,
            &input
        ));
    }

    #[test]
    fn runtime_executor_restarts_once_on_cpu_before_output() {
        let mut attempts = Vec::new();
        let mut fallbacks = 0;
        let execution = execute_runtime_with_cpu_fallback(
            ModeLoaded::new("gpu", ModelLoadMode::GpuOffload),
            ModeLoaded::mode,
            |model| {
                attempts.push(model.mode());
                match model.mode() {
                    ModelLoadMode::GpuOffload => {
                        Err(RuntimeAttemptError::runtime("gpu decode", false))
                    }
                    ModelLoadMode::CpuOnly => Ok("complete"),
                }
            },
            |_| {
                fallbacks += 1;
                Ok(ModeLoaded::new("cpu", ModelLoadMode::CpuOnly))
            },
        );

        assert_eq!(execution.result, Ok("complete"));
        assert_eq!(
            execution.final_model.as_ref().unwrap().mode(),
            ModelLoadMode::CpuOnly
        );
        assert_eq!(fallbacks, 1);
        assert_eq!(
            attempts,
            vec![ModelLoadMode::GpuOffload, ModelLoadMode::CpuOnly]
        );
    }

    #[test]
    fn runtime_executor_never_replays_after_visible_output_or_input_error() {
        for failure in [
            RuntimeAttemptError::runtime("after output", true),
            RuntimeAttemptError::input("bad input"),
        ] {
            let mut fallbacks = 0;
            let mut failure = Some(failure);
            let execution = execute_runtime_with_cpu_fallback(
                ModeLoaded::new("gpu", ModelLoadMode::GpuOffload),
                ModeLoaded::mode,
                |_| -> std::result::Result<(), RuntimeAttemptError<&str>> {
                    Err(failure.take().unwrap())
                },
                |_| {
                    fallbacks += 1;
                    Ok(ModeLoaded::new("cpu", ModelLoadMode::CpuOnly))
                },
            );

            assert!(execution.result.is_err());
            assert_eq!(
                execution.final_model.as_ref().unwrap().mode(),
                ModelLoadMode::GpuOffload
            );
            assert_eq!(fallbacks, 0);
        }
    }

    #[test]
    fn runtime_executor_returns_cpu_attempt_error_without_third_attempt() {
        let mut attempts = 0;
        let execution = execute_runtime_with_cpu_fallback(
            ModeLoaded::new("gpu", ModelLoadMode::GpuOffload),
            ModeLoaded::mode,
            |model| -> std::result::Result<(), RuntimeAttemptError<&str>> {
                attempts += 1;
                Err(RuntimeAttemptError::runtime(
                    if model.mode().uses_gpu() {
                        "gpu failed"
                    } else {
                        "cpu failed"
                    },
                    false,
                ))
            },
            |_| Ok(ModeLoaded::new("cpu", ModelLoadMode::CpuOnly)),
        );

        assert_eq!(execution.result.unwrap_err(), "cpu failed");
        assert_eq!(attempts, 2);
    }

    #[test]
    fn cache_records_mode_and_reuses_existing_cpu_replacement() {
        let cache = InferenceModelCache::new();
        let lease = cache.acquire();
        let gpu = lease
            .get_or_try_init(|| Ok::<_, ()>(ModeLoaded::new("gpu", ModelLoadMode::GpuOffload)))
            .unwrap();
        assert_eq!(cache.cached_mode(), Some(ModelLoadMode::GpuOffload));

        let cpu = lease
            .replace_gpu_with_cpu(gpu, || Ok::<_, ()>("cpu"))
            .unwrap();
        assert_eq!(cpu.mode(), ModelLoadMode::CpuOnly);
        assert_eq!(*cpu.value(), "cpu");
        drop(cpu);
        let reused = lease
            .get_or_try_init(|| -> Result<ModeLoaded<&str>, ()> {
                panic!("the cached CPU replacement must be reused")
            })
            .unwrap();

        assert_eq!(*reused.value(), "cpu");
        assert_eq!(cache.cached_mode(), Some(ModelLoadMode::CpuOnly));
    }

    #[test]
    #[should_panic(expected = "only one live model handle")]
    fn lease_rejects_a_second_live_model_handle() {
        let cache = InferenceModelCache::new();
        let lease = cache.acquire();
        let _first = lease
            .get_or_try_init(|| Ok::<_, ()>(ModeLoaded::new("gpu", ModelLoadMode::GpuOffload)))
            .unwrap();
        let _second = lease
            .get_or_try_init(|| -> Result<ModeLoaded<&str>, ()> {
                panic!("the cached model should be reused")
            })
            .unwrap();
    }

    #[test]
    fn concurrent_requests_serialize_fallback_and_reuse_one_cpu_model() {
        #[derive(Debug)]
        struct FakeModel(Option<Arc<AtomicUsize>>);

        impl Drop for FakeModel {
            fn drop(&mut self) {
                if let Some(drops) = self.0.as_ref() {
                    drops.fetch_add(1, Ordering::SeqCst);
                }
            }
        }

        let cache = Arc::new(InferenceModelCache::new());
        let loads = Arc::new(AtomicUsize::new(0));
        let gpu_drops = Arc::new(AtomicUsize::new(0));
        let (held_tx, held_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (attempt_tx, attempt_rx) = mpsc::channel();

        let first_cache = Arc::clone(&cache);
        let first_loads = Arc::clone(&loads);
        let first_drops = Arc::clone(&gpu_drops);
        let fallback_drops = Arc::clone(&gpu_drops);
        let first = std::thread::spawn(move || {
            let lease = first_cache.acquire();
            let gpu = lease
                .get_or_try_init(|| {
                    Ok::<_, ()>(ModeLoaded::new(
                        FakeModel(Some(first_drops)),
                        ModelLoadMode::GpuOffload,
                    ))
                })
                .unwrap();
            held_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            let cpu = lease
                .replace_gpu_with_cpu(gpu, || {
                    assert_eq!(fallback_drops.load(Ordering::SeqCst), 1);
                    first_loads.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, ()>(FakeModel(None))
                })
                .unwrap();
            cpu.mode()
        });

        held_rx.recv().unwrap();
        let second_cache = Arc::clone(&cache);
        let second = std::thread::spawn(move || {
            attempt_tx.send(()).unwrap();
            let lease = second_cache.acquire();
            let cpu = lease
                .get_or_try_init(|| -> Result<ModeLoaded<FakeModel>, ()> {
                    panic!("the serialized request must reuse the CPU replacement")
                })
                .unwrap();
            cpu.mode()
        });
        attempt_rx.recv().unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 0);
        assert_eq!(gpu_drops.load(Ordering::SeqCst), 0);
        release_tx.send(()).unwrap();

        assert_eq!(first.join().unwrap(), ModelLoadMode::CpuOnly);
        assert_eq!(second.join().unwrap(), ModelLoadMode::CpuOnly);
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        assert_eq!(cache.cached_mode(), Some(ModelLoadMode::CpuOnly));
    }

    #[test]
    fn fallback_after_unload_is_detached_and_does_not_repopulate_cache() {
        let cache = InferenceModelCache::new();
        let lease = cache.acquire();
        let gpu = lease
            .get_or_try_init(|| Ok::<_, ()>(ModeLoaded::new("gpu", ModelLoadMode::GpuOffload)))
            .unwrap();
        cache.unload();

        let detached = lease
            .replace_gpu_with_cpu(gpu, || Ok::<_, ()>("cpu"))
            .unwrap();

        assert_eq!(detached.mode(), ModelLoadMode::CpuOnly);
        assert_eq!(cache.cached_mode(), None);
    }

    #[test]
    fn exact_gpu_generation_is_released_before_cpu_load_and_failure_leaves_cache_empty() {
        #[derive(Debug)]
        struct DropProbe(Rc<Cell<usize>>);

        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }

        let drops = Rc::new(Cell::new(0));
        let cache = InferenceModelCache::new();
        let lease = cache.acquire();
        let gpu = lease
            .get_or_try_init(|| {
                Ok::<_, &str>(ModeLoaded::new(
                    DropProbe(Rc::clone(&drops)),
                    ModelLoadMode::GpuOffload,
                ))
            })
            .unwrap();

        let error = lease
            .replace_gpu_with_cpu(gpu, || {
                assert_eq!(drops.get(), 1, "GPU cache must be released first");
                Err::<DropProbe, _>("CPU load failed")
            })
            .unwrap_err();

        assert_eq!(error, "CPU load failed");
        assert_eq!(cache.cached_mode(), None);
    }

    #[test]
    fn cpu_load_does_not_hold_state_lock_and_unload_prevents_repopulation() {
        let cache = InferenceModelCache::new();
        let lease = cache.acquire();
        let gpu = lease
            .get_or_try_init(|| Ok::<_, ()>(ModeLoaded::new("gpu", ModelLoadMode::GpuOffload)))
            .unwrap();
        let detached = lease
            .replace_gpu_with_cpu(gpu, || {
                cache.unload();
                Ok::<_, ()>("cpu")
            })
            .unwrap();

        assert_eq!(detached.mode(), ModelLoadMode::CpuOnly);
        assert_eq!(cache.cached_mode(), None);
        lease.clear_if_current(&detached);
        assert_eq!(cache.cached_mode(), None);
    }

    #[test]
    fn initial_load_does_not_hold_state_lock_and_honors_unload() {
        let cache = InferenceModelCache::new();
        let lease = cache.acquire();
        let detached = lease
            .get_or_try_init(|| {
                cache.unload();
                Ok::<_, ()>(ModeLoaded::new("cpu", ModelLoadMode::CpuOnly))
            })
            .unwrap();

        assert_eq!(detached.mode(), ModelLoadMode::CpuOnly);
        assert_eq!(cache.cached_mode(), None);
    }

    #[test]
    fn unload_after_use_clears_the_current_cpu_generation() {
        let cache = InferenceModelCache::new();
        let lease = cache.acquire();
        let gpu = lease
            .get_or_try_init(|| Ok::<_, ()>(ModeLoaded::new("gpu", ModelLoadMode::GpuOffload)))
            .unwrap();
        let cpu = lease
            .replace_gpu_with_cpu(gpu, || Ok::<_, ()>("cpu"))
            .unwrap();

        lease.clear_if_current(&cpu);
        assert_eq!(cache.cached_mode(), None);
    }

    #[test]
    fn invalid_gguf_is_rejected_before_model_loading() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"not a gguf").unwrap();

        let error = validate_gguf_header(file.path(), "本地模型加载失败").unwrap_err();

        assert!(matches!(error, ProviderError::InvalidRequest(_)));
        assert!(error.to_string().contains("不是有效的 GGUF"));
        assert!(
            !error
                .to_string()
                .contains(&file.path().display().to_string())
        );
    }

    #[test]
    fn capabilities_report_streaming() {
        let llm = LlamaLlm::new(PathBuf::from("/tmp/x.gguf"), LoadPolicy::UnloadAfterUse);
        assert!(llm.capabilities().streaming);
    }

    #[test]
    fn local_thinking_directive_defaults_to_no_think() {
        assert_eq!(
            append_thinking_directive("测试", false),
            "测试\n\n/no_think"
        );
    }

    #[test]
    fn local_thinking_directive_can_enable_thinking() {
        assert_eq!(append_thinking_directive("测试", true), "测试\n\n/think");
    }

    #[test]
    fn visible_output_boundary_is_after_thinking_filter() {
        let mut output = VisibleOutputTracker::default();

        assert_eq!(output.push("<thi"), None);
        assert_eq!(output.push("nk>hidden</think>"), None);
        assert!(!output.has_emitted());
        assert_eq!(output.push("visible"), Some("visible".into()));
        assert!(output.has_emitted());
    }

    #[test]
    fn with_thinking_sets_local_model_flag() {
        let llm =
            LlamaLlm::new(PathBuf::from("/tmp/x.gguf"), LoadPolicy::Resident).with_thinking(true);
        assert!(llm.enable_thinking);
    }

    #[test]
    fn unload_clears_resident_model_slot() {
        let llm = LlamaLlm::new(PathBuf::from("/tmp/x.gguf"), LoadPolicy::Resident);
        llm.unload(); // 未加载时也应安全
        assert_eq!(llm.model.cached_mode(), None);
    }
}
