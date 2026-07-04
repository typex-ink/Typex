//! 托盘图标 + 菜单（M0 基础版：设置/暂停/退出；CP-4.3 补全完整菜单）。

use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager, Runtime,
};

pub fn setup<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let status = MenuItemBuilder::with_id("status", "● Typex 就绪").enabled(false).build(app)?;
    let pause = MenuItemBuilder::with_id("pause", "暂停 Typex").build(app)?;
    let settings = MenuItemBuilder::with_id("settings", "设置…").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&status)
        .separator()
        .item(&pause)
        .separator()
        .item(&settings)
        .item(&quit)
        .build()?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("bundled icon"))
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quit" => app.exit(0),
            "pause" => {
                if let Some(paused) = app.try_state::<crate::app::PausedState>() {
                    let cur = *paused.0.borrow();
                    let _ = paused.0.send(!cur);
                    tracing::info!("暂停切换: {}", !cur);
                }
            }
            "settings" => {
                let _ = crate::app::windows::show_settings(app);
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}
