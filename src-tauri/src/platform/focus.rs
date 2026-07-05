//! 目标应用识别（02 F-7 历史字段 / F-11 预留；CP-6.8）。
//!
//! macOS：NSWorkspace.frontmostApplication 读取前台应用本地化名。
//! 其他平台返回 None（trait 扩展位随平台后端补）。

/// 当前前台应用名（注入目标；录音开始时采样）。
pub fn frontmost_app_name() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWorkspace;
        let ws = NSWorkspace::sharedWorkspace();
        let app = ws.frontmostApplication()?;
        app.localizedName().map(|n| n.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
