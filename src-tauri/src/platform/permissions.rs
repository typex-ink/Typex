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

/// 请求首次授权；已经拒绝或不支持应用内请求时打开系统权限设置页。
pub async fn open_settings(kind: PermissionKind) {
    #[cfg(target_os = "macos")]
    {
        if kind == PermissionKind::Microphone {
            match microphone_permission_action(microphone_authorization_status()) {
                MicrophonePermissionAction::Granted => return,
                MicrophonePermissionAction::Request => {
                    request_microphone_access().await;
                    return;
                }
                MicrophonePermissionAction::OpenSettings => {}
            }
        }

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

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MicrophonePermissionAction {
    Granted,
    Request,
    OpenSettings,
}

#[cfg(target_os = "macos")]
fn microphone_authorization_status() -> objc2_av_foundation::AVAuthorizationStatus {
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

    // SAFETY: AVMediaTypeAudio 是系统提供的 extern 常量；该方法只读取当前 TCC 状态。
    unsafe {
        let Some(media_type) = AVMediaTypeAudio else {
            return AVAuthorizationStatus::Restricted;
        };
        AVCaptureDevice::authorizationStatusForMediaType(media_type)
    }
}

#[cfg(target_os = "macos")]
fn microphone_permission_action(
    status: objc2_av_foundation::AVAuthorizationStatus,
) -> MicrophonePermissionAction {
    use objc2_av_foundation::AVAuthorizationStatus;

    match status {
        AVAuthorizationStatus::Authorized => MicrophonePermissionAction::Granted,
        AVAuthorizationStatus::NotDetermined => MicrophonePermissionAction::Request,
        _ => MicrophonePermissionAction::OpenSettings,
    }
}

#[cfg(target_os = "macos")]
fn begin_microphone_access_request() -> Option<tokio::sync::oneshot::Receiver<bool>> {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};
    use std::sync::Mutex;

    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Mutex::new(Some(tx));
    let handler = RcBlock::new(move |granted: Bool| {
        if let Ok(mut tx) = tx.lock()
            && let Some(tx) = tx.take()
        {
            let _ = tx.send(granted.as_bool());
        }
    });

    // SAFETY: AVFoundation copies the escaping block and invokes it once on an arbitrary queue.
    // The captured sender is Send, synchronized by a Mutex, and performs no UI work.
    unsafe {
        let Some(media_type) = AVMediaTypeAudio else {
            tracing::warn!("AVMediaTypeAudio unavailable; microphone permission was not requested");
            return None;
        };
        AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type, &handler);
    }
    Some(rx)
}

#[cfg(target_os = "macos")]
async fn request_microphone_access() {
    let Some(rx) = begin_microphone_access_request() else {
        return;
    };
    match rx.await {
        Ok(granted) => tracing::info!(granted, "microphone permission request completed"),
        Err(error) => tracing::warn!(%error, "microphone permission request callback dropped"),
    }
}

/// 麦克风权限（macOS：AVCaptureDevice authorizationStatus）。
#[cfg(target_os = "macos")]
fn microphone_granted() -> bool {
    microphone_authorization_status() == objc2_av_foundation::AVAuthorizationStatus::Authorized
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use objc2_av_foundation::AVAuthorizationStatus;

    #[test]
    fn microphone_permission_action_matches_tcc_state() {
        assert_eq!(
            microphone_permission_action(AVAuthorizationStatus::Authorized),
            MicrophonePermissionAction::Granted
        );
        assert_eq!(
            microphone_permission_action(AVAuthorizationStatus::NotDetermined),
            MicrophonePermissionAction::Request
        );
        assert_eq!(
            microphone_permission_action(AVAuthorizationStatus::Denied),
            MicrophonePermissionAction::OpenSettings
        );
        assert_eq!(
            microphone_permission_action(AVAuthorizationStatus::Restricted),
            MicrophonePermissionAction::OpenSettings
        );
    }
}
