//! 窗口创建/显隐/定位（07 §2）。HUD NSPanel 处理在 CP-1.3。

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// 设置窗口：720×520 常规窗口，按需创建（05 §5）。
pub fn show_settings<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("settings") {
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("src/windows/settings/index.html".into()))
        .title("Typex 设置")
        .inner_size(720.0, 520.0)
        .resizable(false)
        .build()?;
    Ok(())
}
