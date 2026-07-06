//! 本地 LLM Provider · llama.cpp + Qwen3.5 GGUF（CP-8.5 / 03 §3.3 / ADR-20/22）。
//!
//! 实现同一 `LlmProvider` trait（流式 delta 与云端一致）：推理在专属线程逐
//! token 生成，经 tokio mpsc channel 转成 BoxStream。上下文 4K（整理/翻译都是
//! 短输入）；chat 模板优先用模型内置（GGUF metadata），缺失时手拼 Qwen ChatML。
//! 错误分类只剩 InvalidRequest / 模型未下载（03 §3.3）。

use crate::providers::ProviderError;
use crate::providers::llm::{
    LlmCapabilities, LlmDelta, LlmProvider, LlmRequest, filter_thinking_stream,
};
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

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

/// 运行时加载策略（03 §3.3：常驻内存 / 用完即卸，设置可选）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPolicy {
    /// 常驻内存：首次加载后保留（冷加载 1–3s 只付一次）。
    Resident,
    /// 用完即卸：每次 complete 结束后释放（省内存，每次付冷加载）。
    UnloadAfterUse,
}

pub struct LlamaLlm {
    /// 惰性加载；Arc 使生成线程可持有引用而不阻塞 unload。
    model: Arc<Mutex<Option<Arc<LlamaModel>>>>,
    model_path: PathBuf,
    policy: LoadPolicy,
    enable_thinking: bool,
}

impl LlamaLlm {
    /// `model_path` = `{data_dir}/models/{model_id}/qwen3.5-*.gguf`（构造函数注入）。
    pub fn new(model_path: PathBuf, policy: LoadPolicy) -> Self {
        Self {
            model: Arc::new(Mutex::new(None)),
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
        *self.model.lock().unwrap() = None;
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
    fn ensure_loaded(
        slot: &Mutex<Option<Arc<LlamaModel>>>,
        path: &PathBuf,
    ) -> Result<Arc<LlamaModel>, ProviderError> {
        let mut guard = slot.lock().unwrap();
        if let Some(model) = guard.as_ref() {
            return Ok(Arc::clone(model));
        }
        let params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(llama_backend(), path, &params)
            .map_err(|e| ProviderError::InvalidRequest(format!("本地 LLM 模型加载失败: {e}")))?;
        let model = Arc::new(model);
        *guard = Some(Arc::clone(&model));
        Ok(model)
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

/// 组装 prompt：优先模型内置 chat 模板（GGUF metadata），缺失时手拼 Qwen ChatML。
fn build_prompt(
    model: &LlamaModel,
    req: &LlmRequest,
    enable_thinking: bool,
) -> Result<String, ProviderError> {
    let invalid = |e: &dyn std::fmt::Display| ProviderError::InvalidRequest(format!("{e}"));
    let last_user = req.messages.iter().rposition(|m| m.role == "user");
    if let Ok(template) = model.chat_template(None) {
        let mut chat = Vec::with_capacity(req.messages.len() + 1);
        if !req.system.is_empty() {
            chat.push(
                LlamaChatMessage::new("system".into(), req.system.clone())
                    .map_err(|e| invalid(&e))?,
            );
        }
        for (idx, m) in req.messages.iter().enumerate() {
            let content = if Some(idx) == last_user {
                append_thinking_directive(&m.content, enable_thinking)
            } else {
                m.content.clone()
            };
            chat.push(LlamaChatMessage::new(m.role.clone(), content).map_err(|e| invalid(&e))?);
        }
        return model
            .apply_chat_template(&template, &chat, true)
            .map_err(|e| invalid(&e));
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
    req: &LlmRequest,
    enable_thinking: bool,
    tx: &tokio::sync::mpsc::UnboundedSender<Result<LlmDelta, ProviderError>>,
) -> Result<(), ProviderError> {
    let invalid = |e: &dyn std::fmt::Display| ProviderError::InvalidRequest(format!("{e}"));

    let prompt = build_prompt(model, req, enable_thinking)?;
    // 模板已含特殊 token，不再加 BOS
    let tokens = model
        .str_to_token(&prompt, AddBos::Never)
        .map_err(|e| invalid(&e))?;
    let max_new = req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
    if tokens.len() as u32 + max_new > N_CTX {
        return Err(ProviderError::InvalidRequest(format!(
            "输入过长：{} tokens 超出本地模型 {N_CTX} 上下文窗口",
            tokens.len()
        )));
    }

    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(N_CTX))
        .with_n_batch(N_CTX);
    let mut ctx = model
        .new_context(llama_backend(), ctx_params)
        .map_err(|e| invalid(&e))?;

    let mut batch = LlamaBatch::new(N_CTX as usize, 1);
    batch
        .add_sequence(&tokens, 0, false)
        .map_err(|e| invalid(&e))?;
    ctx.decode(&mut batch).map_err(|e| invalid(&e))?;

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
    let first_pos = tokens.len() as i32;
    for n_cur in first_pos..first_pos + max_new as i32 {
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        if model.is_eog_token(token) {
            break;
        }
        let piece = model
            .token_to_piece(token, &mut decoder, false, None)
            .map_err(|e| invalid(&e))?;
        if !piece.is_empty() && tx.send(Ok(LlmDelta { text: piece })).is_err() {
            break; // 下游已丢弃 stream（会话取消）——停止生成
        }
        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|e| invalid(&e))?;
        ctx.decode(&mut batch).map_err(|e| invalid(&e))?;
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
            let result = Self::ensure_loaded(&slot, &path)
                .and_then(|model| generate_blocking(&model, &req, enable_thinking, &tx));
            if let Err(e) = result {
                let _ = tx.send(Err(e));
            }
            if policy == LoadPolicy::UnloadAfterUse {
                *slot.lock().unwrap() = None;
            }
        });
        let stream = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        filter_thinking_stream(stream)
    }

    fn capabilities(&self) -> LlmCapabilities {
        LlmCapabilities { streaming: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn with_thinking_sets_local_model_flag() {
        let llm =
            LlamaLlm::new(PathBuf::from("/tmp/x.gguf"), LoadPolicy::Resident).with_thinking(true);
        assert!(llm.enable_thinking);
    }

    #[test]
    fn unload_clears_resident_model_slot() {
        let llm = LlamaLlm::new(PathBuf::from("/tmp/x.gguf"), LoadPolicy::Resident);
        llm.unload(); // 未加载时也应安全
        assert!(llm.model.lock().unwrap().is_none());
    }
}
