//! app 层（Tauri 壳）：只做胶水，不写业务逻辑（06 §3 规则 2）。
pub mod commands;
pub mod events;
pub mod tray;
pub mod tray_icon;
pub mod windows;

/// 「暂停 Typex」全局状态（托盘切换；hotkey 线程订阅）。
pub struct PausedState(pub tokio::sync::watch::Sender<bool>);

/// 最近一次结果（内存级；托盘「复制上次结果」，02 F-7）。与 orchestrator 共享。
pub struct LastResult(pub std::sync::Arc<std::sync::Mutex<Option<String>>>);

/// 进行中的本地模型下载任务（model_id → 任务句柄；local-models）。
/// 无条件定义：默认构建下 download_local_model 返回错误，map 恒为空。
#[derive(Default)]
pub struct LocalDownloads(
    pub std::sync::Mutex<std::collections::HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
);
