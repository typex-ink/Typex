//! 目标应用识别（02 F-7 历史字段 / 03 §3.4 prompt 上下文 / F-11 预留）。
//!
//! macOS：NSWorkspace.frontmostApplication 读取前台应用本地化名。
//! Windows / Linux 适配时补齐对应前台应用探测；当前非 macOS 返回 None。

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
