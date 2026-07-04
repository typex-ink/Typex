//! 全部 #[tauri::command]（薄，仅转发；完整清单见 07 §10.1，按里程碑逐步补齐）。

use crate::settings::{schema::Settings, SettingsService};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
#[specta::specta]
pub fn get_settings(settings: State<'_, Arc<SettingsService>>) -> Settings {
    settings.get()
}

#[tauri::command]
#[specta::specta]
pub fn update_settings(
    settings: State<'_, Arc<SettingsService>>,
    new_settings: Settings,
) -> Result<Settings, crate::error::TypexError> {
    settings.update(new_settings)
}

#[tauri::command]
#[specta::specta]
pub fn get_permission_status() -> Vec<crate::platform::permissions::PermissionStatus> {
    crate::platform::permissions::check_all()
}

#[tauri::command]
#[specta::specta]
pub fn session_command(
    commander: State<'_, crate::orchestrator::SessionCommander>,
    command: crate::orchestrator::SessionCommand,
) {
    let _ = commander.0.send(command);
}
