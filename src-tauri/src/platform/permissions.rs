//! 各平台权限检测/引导（07 §4 platform/permissions.rs）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    Microphone,
    Accessibility,
    InputMonitoring,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct PermissionStatus {
    pub kind: PermissionKind,
    pub granted: bool,
}

/// 打开系统权限设置页（onboarding「去授权」按钮）。
pub fn open_settings(kind: PermissionKind) {
    #[cfg(target_os = "macos")]
    {
        let pane = match kind {
            PermissionKind::Microphone => "Privacy_Microphone",
            PermissionKind::Accessibility => "Privacy_Accessibility",
            PermissionKind::InputMonitoring => "Privacy_ListenEvent",
        };
        let _ = std::process::Command::new("open")
            .arg(format!(
                "x-apple.systempreferences:com.apple.preference.security?{pane}"
            ))
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = kind;
}

/// 检测全部权限状态（macOS 主动检测；其他平台按需扩展）。
pub fn check_all() -> Vec<PermissionStatus> {
    #[cfg(target_os = "macos")]
    {
        vec![
            PermissionStatus {
                kind: PermissionKind::Accessibility,
                granted: macos_accessibility_client::accessibility::application_is_trusted(),
            },
            // 麦克风与输入监听的检测在 CP-1.8 onboarding 完善
        ]
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}
