//! 本地模型子系统（v1.1 / F-12 / ADR-20 / ADR-22）。
//!
//! 全部代码锁在 Cargo feature `local-models` 后（07 §1）；
//! 默认构建不编译、不拉入任何推理引擎依赖。
//!
//! 子模块：
//! - [`manifest`]：内置模型清单（6 条目，ADR-22）
//! - [`hardware`]：RAM / CPU / GPU 探测 → 轻量 / 标准 / 性能三档
//! - [`download`]：HTTP Range 断点续传、SHA-256 校验、换源重试
//!
//! Tauri commands 注册暂缓（默认构建必须零影响）；
//! v1.1 开启 feature 时接入 runner，见 src-tauri/src/runner.rs 中的 TODO 注释。

pub mod download;
pub mod hardware;
pub mod manifest;
