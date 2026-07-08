//! 本地模型子系统（F-12 / ADR-20 / ADR-22）。
//!
//! 全部代码锁在 Cargo feature `local-models` 后（06 §1）；
//! 默认 feature 已启用，需要裁剪推理引擎依赖时可用 `--no-default-features`。
//!
//! 子模块：
//! - [`manifest`]：内置模型清单（ADR-22）
//! - [`hardware`]：RAM / CPU / GPU 探测 → 轻量 / 标准 / 性能三档
//! - [`import`]：用户已下载模型的托管导入
//! - [`download`]：HTTP Range 断点续传、SHA-256 校验、换源重试
//! - [`stt_sense_voice`]：SenseVoice 轻量档 STT（sherpa-onnx）
//! - [`stt_qwen_asr`]：Qwen3-ASR 标准/性能档 STT（llama.cpp mtmd）
//! - [`stt_whisper`]：Whisper large-v3 高配精度档 STT（sherpa-onnx）
//! - [`llm_llama`]：Qwen3.5 本地 LLM，整理/翻译槽（llama.cpp）
//!
//! IPC 契约无条件注册，具体实现按 feature 分支执行。

pub mod download;
pub mod hardware;
pub mod import;
pub mod llm_llama;
pub mod manifest;
pub mod stt_qwen_asr;
pub mod stt_sense_voice;
pub mod stt_whisper;
