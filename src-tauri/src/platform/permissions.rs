//! 各平台权限检测/引导（06 §4 platform/permissions.rs）。

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
    #[cfg(target_os = "windows")]
    {
        if kind == PermissionKind::Microphone {
            let _ = super::shell::open_uri("ms-settings:privacy-microphone");
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = kind;
}

/// 麦克风权限（macOS：AVCaptureDevice authorizationStatus，3 = Authorized）。
/// NotDetermined 时首次开流会触发系统弹窗——按未授权报告，onboarding 引导点击。
#[cfg(target_os = "macos")]
fn microphone_granted() -> bool {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
    // SAFETY: AVMediaTypeAudio 是系统常量；authorizationStatusForMediaType 无副作用
    unsafe {
        let Some(media_type) = AVMediaTypeAudio else {
            return false;
        };
        AVCaptureDevice::authorizationStatusForMediaType(media_type)
            == AVAuthorizationStatus::Authorized
    }
}

/// 输入监听权限（macOS：IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)==granted）。
#[cfg(target_os = "macos")]
fn input_monitoring_granted() -> bool {
    // IOHIDRequestTypeListenEvent = 1；kIOHIDAccessTypeGranted = 0
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOHIDCheckAccess(request_type: u32) -> u32;
    }
    // SAFETY: 纯查询 API，无副作用
    unsafe { IOHIDCheckAccess(1) == 0 }
}

/// 检测全部权限状态。
///
/// Windows 桌面应用只有麦克风隐私总开关；快捷键与注入不需要预授权。
pub fn check_all() -> Vec<PermissionStatus> {
    #[cfg(target_os = "macos")]
    {
        vec![
            PermissionStatus {
                kind: PermissionKind::Microphone,
                granted: microphone_granted(),
            },
            PermissionStatus {
                kind: PermissionKind::Accessibility,
                granted: macos_accessibility_client::accessibility::application_is_trusted(),
            },
            PermissionStatus {
                kind: PermissionKind::InputMonitoring,
                granted: input_monitoring_granted(),
            },
        ]
    }
    #[cfg(target_os = "windows")]
    {
        vec![PermissionStatus {
            kind: PermissionKind::Microphone,
            granted: super::windows::microphone_access_allowed(),
        }]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}
