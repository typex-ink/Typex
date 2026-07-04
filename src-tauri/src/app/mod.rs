//! app 层（Tauri 壳）：只做胶水，不写业务逻辑（07 §3 规则 2）。
pub mod commands;
pub mod events;
pub mod tray;
pub mod windows;

/// 「暂停 Typex」全局状态（托盘切换；hotkey 线程订阅）。
pub struct PausedState(pub tokio::sync::watch::Sender<bool>);
