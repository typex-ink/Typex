//! 全部 IPC event 定义与 emit 封装（06 §10.2）。
//! 命名规范：`域://kebab-case`；载荷全部为 types/ 中的 struct。

use crate::types::SessionSnapshot;
use serde::{Deserialize, Serialize};
use tauri_specta::Event;

/// `session://snapshot` — HUD/托盘渲染依据。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct SessionSnapshotEvent(pub SessionSnapshot);

/// `session://audio-level` — HUD 波形（50ms 节流）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct AudioLevelEvent(pub Vec<f32>);

/// `settings://changed` — 全窗口设置同步。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct SettingsChangedEvent(pub crate::settings::schema::Settings);

/// `assistant://started` — 回答弹窗重置内容 + 指令回显（回答型确认的那一刻）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct AssistantStartedEvent {
    pub request_id: u32,
    /// 本次语音指令（弹窗顶部回显）
    pub instruction: String,
    /// 选区字数（摘要行显示；None = 无选区）
    pub selection_chars: Option<u32>,
    /// 读取选区失败降级为普通提问（弹窗提示行，05 §4）
    pub degraded: bool,
}

/// `assistant://delta` — 回答弹窗流式渲染。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct AssistantDeltaEvent {
    pub request_id: u32,
    pub text_delta: String,
}

/// `assistant://done` — 回答终态（改写型结果不经 assistant:// 事件，走 session 注入）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct AssistantDoneEvent {
    pub request_id: u32,
    pub full_text: String,
}

/// `assistant://error` — 弹窗已呼出后的流中断（此前的失败走 HUD）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct AssistantErrorEvent {
    pub request_id: u32,
    pub error: crate::error::TypexError,
}

/// `update://available` — 启动自动检查发现新版本（安装仍需用户确认，ADR-11）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct UpdateAvailableEvent {
    pub version: String,
    pub notes: String,
}

/// `local://download-progress` — 本地模型下载进度。
/// 事件类型无条件定义（IPC 契约不随 feature 变化）；
/// 默认构建下 download_local_model 返回错误，此事件不会发出。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
pub struct LocalDownloadProgressEvent {
    /// 模型库条目 id（manifest）。
    pub model_id: String,
    /// 已下载字节（跨文件累计，含续传偏移）。
    pub bytes_done: u64,
    /// 全部文件合计字节（来自 manifest）。
    pub bytes_total: u64,
    /// true = 终态（成功或失败，见 error）。
    pub done: bool,
    /// 终态错误信息（None = 成功；"cancelled" = 用户取消）。
    pub error: Option<String>,
}
