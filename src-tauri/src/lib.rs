// Typex — 模块树遵循 docs/07-code-architecture.md §4
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

pub use runner::run;
