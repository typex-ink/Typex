//! 目标应用识别（02 F-7 历史字段 / 03 §3.4 prompt 上下文 / F-11 预留）。
//!
//! macOS：NSWorkspace.frontmostApplication 读取前台应用本地化名。
//! Windows：GetForegroundWindow + PID 对应的进程映像名（从不读取窗口标题）。

/// Opaque in-process injection target captured when recording starts.
/// Native handles are never serialized, logged, or exposed over IPC.
#[derive(Debug, Clone)]
pub struct FocusTarget {
    app_name: Option<String>,
    #[cfg(target_os = "windows")]
    window_id: isize,
    #[cfg(target_os = "windows")]
    process_id: u32,
}

impl FocusTarget {
    pub fn capture() -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::NSWorkspace;
            let ws = NSWorkspace::sharedWorkspace();
            let app = ws.frontmostApplication()?;
            Some(Self {
                app_name: app.localizedName().map(|name| name.to_string()),
            })
        }
        #[cfg(target_os = "windows")]
        {
            let window = super::windows::foreground_window()?;
            Some(Self {
                app_name: window.app_name,
                window_id: window.hwnd.0 as isize,
                process_id: window.process_id,
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            None
        }
    }

    pub fn app_name(&self) -> Option<String> {
        self.app_name.clone()
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn windows_process_id(&self) -> u32 {
        self.process_id
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn windows_window_id(&self) -> isize {
        self.window_id
    }

    /// Windows requires the exact captured HWND/PID pair. Other platforms retain their existing
    /// injection behavior until they expose an equivalent stable native identity.
    pub fn is_current(&self) -> bool {
        #[cfg(target_os = "windows")]
        {
            super::windows::foreground_window().is_some_and(|window| {
                same_windows_identity(
                    self.window_id,
                    self.process_id,
                    window.hwnd.0 as isize,
                    window.process_id,
                )
            })
        }
        #[cfg(not(target_os = "windows"))]
        {
            true
        }
    }
}

/// Current foreground application name for non-session callers.
pub fn frontmost_app_name() -> Option<String> {
    FocusTarget::capture().and_then(|target| target.app_name())
}

/// A missing Windows capture is not a license to inject into whichever window appears later.
pub fn captured_target_is_current(target: Option<&FocusTarget>) -> bool {
    #[cfg(target_os = "windows")]
    {
        target.is_some_and(FocusTarget::is_current)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        true
    }
}

#[cfg(target_os = "windows")]
fn same_windows_identity(
    captured_window: isize,
    captured_pid: u32,
    current_window: isize,
    current_pid: u32,
) -> bool {
    captured_window == current_window && captured_pid == current_pid
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_identity_requires_both_window_and_process() {
        assert!(super::same_windows_identity(10, 20, 10, 20));
        assert!(!super::same_windows_identity(10, 20, 11, 20));
        assert!(!super::same_windows_identity(10, 20, 10, 21));
    }
}
