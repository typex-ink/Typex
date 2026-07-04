//! 全部 IPC event 定义与 emit 封装（07 §10.2）。
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
