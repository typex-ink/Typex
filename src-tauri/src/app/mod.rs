//! app 层（Tauri 壳）：只做胶水，不写业务逻辑（07 §3 规则 2）。
pub mod commands;
pub mod events;
pub mod tray;
pub mod windows;

/// 「暂停 Typex」全局状态（托盘切换；hotkey 线程订阅）。
pub struct PausedState(pub tokio::sync::watch::Sender<bool>);

/// 最近一次结果（内存级；托盘「复制上次结果」，02 F-7）。与 orchestrator 共享。
pub struct LastResult(pub std::sync::Arc<std::sync::Mutex<Option<String>>>);

/// 助手面板的当前上下文选区（面板打开时读取；ask 时消费）。
pub struct AssistantSelection(pub std::sync::Arc<std::sync::Mutex<Option<String>>>);
