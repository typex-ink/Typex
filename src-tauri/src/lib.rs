// Typex — 模块树遵循 docs/06-code-architecture.md §4
pub mod app;
pub mod audio;
pub mod error;
pub mod history;
pub mod hotkey;
pub mod inject;
pub mod logging;
pub mod orchestrator;
pub mod platform;
pub mod providers;
pub mod runner;
pub mod selection;
pub mod settings;
pub mod types;

// 本地模型子系统（F-12 / ADR-20/22）；由 local-models feature 控制（06 §1）。
#[cfg(feature = "local-models")]
pub mod local;

pub use runner::run;
