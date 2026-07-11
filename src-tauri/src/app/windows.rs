//! 窗口创建/显隐/定位（06 §2）。HUD 使用 nonactivating NSPanel。

use crate::selection::{SelectionBounds, SelectionReader};
use crate::settings::{SettingsService, schema::ThemeMode};
use crate::types::{SessionPhase, SessionSnapshot};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// HUD 显隐代际：每次快照 +1；延迟隐藏只在代际未变时执行（防止杀掉新会话的 HUD）。
static HUD_GEN: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "windows")]
pub fn refresh_windows_window_icons<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let theme = current_theme_mode(window.app_handle());
    set_windows_window_icons(window, windows_window_icon_uses_ink(&theme));
}

#[cfg(target_os = "windows")]
fn windows_window_icon_uses_ink(theme: &ThemeMode) -> bool {
    match theme {
        ThemeMode::Light => true,
        ThemeMode::Dark => false,
        ThemeMode::System => !crate::platform::windows::apps_use_dark_theme(),
    }
}

#[cfg(target_os = "windows")]
fn set_windows_window_icons<R: Runtime>(window: &tauri::WebviewWindow<R>, use_ink: bool) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    if let Err(classification) = crate::platform::windows::configure_app_window_icons(hwnd, use_ink)
    {
        tracing::warn!(classification, "Windows 窗口图标设置失败");
    }
}

#[cfg(target_os = "windows")]
fn queue_windows_frame_redraw<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let raw_hwnd = hwnd.0 as isize;
    let label = window.label().to_owned();
    if let Err(error) = window.run_on_main_thread(move || {
        // DwmSetWindowAttribute returns before the compositor has repainted the non-client area.
        // A short post-message delay keeps the old title color from surviving until reactivation.
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(32)).await;
            let hwnd = windows::Win32::Foundation::HWND(raw_hwnd as *mut std::ffi::c_void);
            if let Err(classification) = crate::platform::windows::redraw_window_frame(hwnd) {
                tracing::warn!(window = label, classification, "Windows 标题栏主题刷新失败");
            }
        });
    }) {
        tracing::warn!(window = window.label(), %error, "Windows 标题栏主题刷新排队失败");
    }
}

fn native_theme(theme: &ThemeMode) -> Option<tauri::Theme> {
    match theme {
        ThemeMode::System => None,
        ThemeMode::Light => Some(tauri::Theme::Light),
        ThemeMode::Dark => Some(tauri::Theme::Dark),
    }
}

fn current_theme_mode<R: Runtime>(app: &AppHandle<R>) -> ThemeMode {
    app.try_state::<Arc<SettingsService>>()
        .map(|settings| settings.get().general.theme)
        .unwrap_or_default()
}

fn current_native_theme<R: Runtime>(app: &AppHandle<R>) -> Option<tauri::Theme> {
    native_theme(&current_theme_mode(app))
}

fn themed_app_url(path: &str, theme: &ThemeMode) -> WebviewUrl {
    let theme = match theme {
        ThemeMode::System => "system",
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
    };
    WebviewUrl::App(format!("{path}?theme={theme}").into())
}

fn chrome_window_background(theme: &ThemeMode) -> tauri::utils::config::Color {
    match effective_theme(theme) {
        // Keep startup background aligned with 04 §3 `--surface-2`.
        tauri::Theme::Dark => tauri::utils::config::Color(0x23, 0x23, 0x27, 0xff),
        tauri::Theme::Light => tauri::utils::config::Color(0xef, 0xef, 0xee, 0xff),
        _ => tauri::utils::config::Color(0xef, 0xef, 0xee, 0xff),
    }
}

fn effective_theme(theme: &ThemeMode) -> tauri::Theme {
    native_theme(theme)
        .or_else(current_system_theme)
        .unwrap_or(tauri::Theme::Light)
}

#[cfg(target_os = "macos")]
fn current_system_theme() -> Option<tauri::Theme> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSAppearanceNameAccessibilityHighContrastDarkAqua, NSAppearanceNameDarkAqua, NSApplication,
    };

    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    let name = app.effectiveAppearance().name();
    let dark = unsafe {
        name.isEqualToString(NSAppearanceNameDarkAqua)
            || name.isEqualToString(NSAppearanceNameAccessibilityHighContrastDarkAqua)
    };
    Some(if dark {
        tauri::Theme::Dark
    } else {
        tauri::Theme::Light
    })
}

#[cfg(not(target_os = "macos"))]
fn current_system_theme() -> Option<tauri::Theme> {
    None
}

/// 同步系统原生窗口外观；macOS 的标题栏/红绿灯区域不会跟随 WebView CSS 自动切换。
pub fn apply_native_theme<R: Runtime>(app: &AppHandle<R>, theme: &ThemeMode) {
    #[cfg(target_os = "windows")]
    let use_ink_icon = windows_window_icon_uses_ink(theme);
    let theme = native_theme(theme);
    app.set_theme(theme);
    for label in ["hud", "home", "settings", "onboarding", "assistant"] {
        if let Some(window) = app.get_webview_window(label) {
            if let Err(error) = window.set_theme(theme) {
                tracing::warn!(window = label, %error, "原生窗口主题设置失败");
                continue;
            }
            #[cfg(target_os = "windows")]
            if matches!(label, "home" | "settings" | "onboarding") {
                set_windows_window_icons(&window, use_ink_icon);
                // Tauri/Tao updates the DWM flag asynchronously; queue the frame repaint after it.
                queue_windows_frame_redraw(&window);
            }
            #[cfg(target_os = "macos")]
            apply_macos_window_chrome(label, &window, theme);
        }
    }
}

/// 系统外观变化时刷新原生 chrome 背景；仅 `general.theme = system` 会收到该事件。
#[cfg(target_os = "macos")]
pub fn refresh_native_chrome<R: Runtime>(app: &AppHandle<R>) {
    let mode = current_theme_mode(app);
    for label in ["home", "settings"] {
        if let Some(window) = app.get_webview_window(label) {
            apply_macos_window_chrome(label, &window, native_theme(&mode));
        }
    }
}

#[cfg(target_os = "macos")]
fn apply_macos_window_chrome<R: Runtime>(
    label: &str,
    window: &tauri::WebviewWindow<R>,
    fixed_theme: Option<tauri::Theme>,
) {
    if !matches!(label, "home" | "settings") {
        return;
    }
    let _ = window.set_title_bar_style(tauri::TitleBarStyle::Transparent);
    let effective_theme = fixed_theme
        .or_else(|| window.theme().ok())
        .unwrap_or(tauri::Theme::Light);
    let (r, g, b) = match effective_theme {
        // Keep AppKit titlebar background pinned to 04 §3 `--surface-2`.
        tauri::Theme::Dark => (0x23, 0x23, 0x27),
        tauri::Theme::Light => (0xef, 0xef, 0xee),
        _ => (0xef, 0xef, 0xee),
    };
    set_macos_window_background(window, r, g, b);
}

#[cfg(target_os = "macos")]
fn set_macos_window_background<R: Runtime>(window: &tauri::WebviewWindow<R>, r: u8, g: u8, b: u8) {
    use objc2_app_kit::{NSColor, NSWindow};

    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    if ns_window.is_null() {
        return;
    }

    unsafe {
        let ns_window = &*ns_window.cast::<NSWindow>();
        let color = NSColor::colorWithDeviceRed_green_blue_alpha(
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
            1.0,
        );
        ns_window.setBackgroundColor(Some(&color));
    }
}

#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    // HUD：nonactivating NSPanel——显示时绝不抢焦点（06 §7.2 坑 3）
    panel!(HudPanel {
        config: {
            can_become_key_window: false,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

/// HUD 窗口 → NSPanel（macOS；启动时调用一次）。
#[cfg(target_os = "macos")]
pub fn setup_hud_panel(app: &AppHandle) -> tauri::Result<()> {
    use tauri_nspanel::{CollectionBehavior, PanelLevel, StyleMask, WebviewWindowExt};
    let Some(hud) = app.get_webview_window("hud") else {
        return Ok(());
    };
    let panel = hud.to_panel::<HudPanel>()?;
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
    panel.set_level(PanelLevel::ScreenSaver.into()); // 全屏应用之上也可见
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .value(),
    );
    Ok(())
}

/// HUD 显隐随会话状态（05 §3.3）。Idle 延迟 700ms 隐藏——给前端 600ms 成功反馈留时间；
/// 期间若新会话开始，show 会先到，隐藏检查会话仍为 Idle 才执行。
pub fn sync_hud_visibility<R: Runtime>(app: &AppHandle<R>, snap: &SessionSnapshot) {
    let Some(hud) = app.get_webview_window("hud") else {
        return;
    };
    let generation = HUD_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    match snap.phase {
        SessionPhase::Idle => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                // 期间有新快照（新会话 show）→ 放弃本次隐藏
                if HUD_GEN.load(Ordering::SeqCst) != generation {
                    return;
                }
                if let Some(hud) = app.get_webview_window("hud") {
                    let _ = hud.hide();
                }
            });
        }
        _ => {
            // A failed HUD may still be visible when a new session starts in another app. Re-read
            // the foreground HWND so Windows follows the new target monitor without a hide/show.
            #[cfg(target_os = "windows")]
            position_hud(&hud);
            if !hud.is_visible().unwrap_or(false) {
                #[cfg(not(target_os = "windows"))]
                position_hud(&hud);
                let _ = hud.show();
            }
        }
    }
}

/// HUD 位置：屏幕底部居中，距底边 48px（05 §3.1）。
#[cfg(target_os = "windows")]
fn position_hud<R: Runtime>(hud: &tauri::WebviewWindow<R>) {
    use crate::platform::windows::{
        PhysicalScreenRect, configure_hud_window, foreground_monitor_work_area, hud_origin_px,
        logical_size_to_physical, place_window_px,
    };

    let hwnd = match hud.hwnd() {
        Ok(hwnd) => hwnd,
        Err(_) => return,
    };
    if let Err(classification) = configure_hud_window(hwnd) {
        tracing::warn!(classification, "Windows HUD 原生样式设置失败");
    }

    let monitor = foreground_monitor_work_area().or_else(|| {
        let monitor = hud.current_monitor().ok().flatten()?;
        let pos = monitor.position();
        let size = monitor.size();
        let work = monitor.work_area();
        Some(crate::platform::windows::MonitorWorkArea {
            monitor_px: PhysicalScreenRect {
                left: pos.x,
                top: pos.y,
                right: pos.x.saturating_add(i32::try_from(size.width).ok()?),
                bottom: pos.y.saturating_add(i32::try_from(size.height).ok()?),
            },
            work_area_px: PhysicalScreenRect {
                left: work.position.x,
                top: work.position.y,
                right: work
                    .position
                    .x
                    .saturating_add(i32::try_from(work.size.width).ok()?),
                bottom: work
                    .position
                    .y
                    .saturating_add(i32::try_from(work.size.height).ok()?),
            },
            scale_factor: monitor.scale_factor(),
        })
    });
    let Some(monitor) = monitor else {
        return;
    };

    // 352×76 is the Tauri logical outer window. The capsule has a 16 DIP transparent inset,
    // so a 32 DIP outer gap preserves the specified 48 DIP visual distance from the work area.
    let size = logical_size_to_physical(352.0, 76.0, monitor.scale_factor);
    let origin = hud_origin_px(monitor.work_area_px, size, monitor.scale_factor, 32.0);
    if let Err(classification) = place_window_px(hwnd, origin, size, true, true) {
        tracing::warn!(classification, "Windows HUD 定位失败");
    }
}

#[cfg(not(target_os = "windows"))]
fn position_hud<R: Runtime>(hud: &tauri::WebviewWindow<R>) {
    if let Ok(Some(monitor)) = hud.current_monitor() {
        let screen = monitor.size();
        let scale = monitor.scale_factor();
        // WebView 外层为 HUD 阴影/毛玻璃预留 16px 透明安全区；
        // 初始定位要扣掉这圈安全区，视觉胶囊底边才仍是 48px。
        let visual_gap = (48.0 - 16.0) * scale;
        let hud_size = hud
            .outer_size()
            .unwrap_or(tauri::PhysicalSize::new(352, 76));
        let x = (screen.width as i32 - hud_size.width as i32) / 2;
        let y = screen.height as i32 - hud_size.height as i32 - visual_gap as i32;
        let _ = hud.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// 设置窗口：720×520 常规窗口，按需创建（05 §5）。
pub fn show_settings<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("settings") {
        #[cfg(target_os = "windows")]
        refresh_windows_window_icons(&w);
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    let theme = current_theme_mode(app);
    let builder = WebviewWindowBuilder::new(
        app,
        "settings",
        themed_app_url("src/windows/settings/index.html", &theme),
    )
    .title("")
    .inner_size(720.0, 520.0)
    .resizable(false)
    .theme(native_theme(&theme))
    .background_color(chrome_window_background(&theme));
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Transparent)
        .hidden_title(true);
    let window = builder.build()?;
    #[cfg(target_os = "windows")]
    refresh_windows_window_icons(&window);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = &window;
    #[cfg(target_os = "macos")]
    apply_macos_window_chrome("settings", &window, native_theme(&theme));
    Ok(())
}

/// 主页窗口：880×560 侧边栏导航（05 §8 / ADR-19）。
pub fn show_home<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("home") {
        #[cfg(target_os = "windows")]
        refresh_windows_window_icons(&w);
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    let theme = current_theme_mode(app);
    let builder = WebviewWindowBuilder::new(
        app,
        "home",
        themed_app_url("src/windows/home/index.html", &theme),
    )
    .title("")
    .inner_size(880.0, 560.0)
    .resizable(false)
    .center()
    .theme(native_theme(&theme))
    .background_color(chrome_window_background(&theme));
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Transparent)
        .hidden_title(true);
    let window = builder.build()?;
    #[cfg(target_os = "windows")]
    refresh_windows_window_icons(&window);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = &window;
    #[cfg(target_os = "macos")]
    apply_macos_window_chrome("home", &window, native_theme(&theme));
    Ok(())
}

/// 回答弹窗：560px 只读浮窗（05 §4 / ADR-23）。
/// `has_selection`：有选区时贴选区下方定位；无选区时屏幕上 1/3 居中。
/// 必须在弹窗获得焦点前读取选区位置，否则焦点已切走、目标应用选区高亮会丢。
pub fn show_assistant<R: Runtime>(app: &AppHandle<R>, has_selection: bool) -> tauri::Result<()> {
    const ASSISTANT_W: f64 = 560.0;
    const ASSISTANT_COMPACT_H: f64 = 136.0;

    let selection_bounds = if has_selection {
        app.try_state::<Arc<dyn SelectionReader>>()
            .as_ref()
            .and_then(|selection| selection.read_bounds().ok().flatten())
    } else {
        None
    };
    if let Some(w) = app.get_webview_window("assistant") {
        let _ = w.set_shadow(false);
        let _ = w.set_resizable(false);
        let was_visible = w.is_visible().unwrap_or(false);
        if was_visible {
            let _ = w.hide();
        }
        let _ = w.set_size(tauri::LogicalSize::new(ASSISTANT_W, ASSISTANT_COMPACT_H));
        position_assistant(app, &w, selection_bounds);
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    let w = WebviewWindowBuilder::new(
        app,
        "assistant",
        WebviewUrl::App("src/windows/assistant/index.html".into()),
    )
    .title("Typex 助手")
    .inner_size(ASSISTANT_W, ASSISTANT_COMPACT_H)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .theme(current_native_theme(app))
    .visible(false)
    .build()?;
    position_assistant(app, &w, selection_bounds);
    w.show()?;
    w.set_focus()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn position_assistant<R: Runtime>(
    app: &AppHandle<R>,
    assistant: &tauri::WebviewWindow<R>,
    selection_bounds: Option<SelectionBounds>,
) {
    use crate::platform::windows::{
        LogicalScreenRect, assistant_origin_px, foreground_monitor_work_area,
        logical_rect_to_physical_screen, logical_size_to_physical, place_window_px,
    };

    let Some(monitor) = foreground_monitor_work_area() else {
        position_assistant_fallback(app, assistant);
        return;
    };
    let size = logical_size_to_physical(560.0, 136.0, monitor.scale_factor);
    // Windows SelectionReader returns screen-space logical DIPs. Convert exactly once with the
    // foreground HWND monitor that supplied the work area.
    let selection_px = selection_bounds.map(|bounds| {
        logical_rect_to_physical_screen(
            LogicalScreenRect {
                x: bounds.x,
                y: bounds.y,
                width: bounds.width,
                height: bounds.height,
            },
            monitor,
        )
    });
    let origin = assistant_origin_px(
        monitor.work_area_px,
        selection_px,
        size,
        monitor.scale_factor,
    );
    if let Ok(hwnd) = assistant.hwnd()
        && place_window_px(hwnd, origin, size, true, true).is_ok()
    {
        return;
    }
    let _ = assistant.set_size(tauri::PhysicalSize::new(
        size.width.max(1) as u32,
        size.height.max(1) as u32,
    ));
    let _ = assistant.set_position(tauri::PhysicalPosition::new(origin.x, origin.y));
}

#[cfg(not(target_os = "windows"))]
fn position_assistant<R: Runtime>(
    app: &AppHandle<R>,
    assistant: &tauri::WebviewWindow<R>,
    selection_bounds: Option<SelectionBounds>,
) {
    const WINDOW_W: f64 = 560.0;
    // 初始弹窗是紧凑回显态；原生窗口预留高度给流式回答，定位避让按可见面板算。
    const VISIBLE_PANEL_H: f64 = 128.0;
    const GAP: f64 = 8.0;
    const MARGIN: f64 = 12.0;

    let Some(bounds) = selection_bounds else {
        position_assistant_fallback(app, assistant);
        return;
    };
    let Ok(monitors) = app.available_monitors() else {
        position_assistant_fallback(app, assistant);
        return;
    };
    let center_x = bounds.x + bounds.width / 2.0;
    let center_y = bounds.y + bounds.height / 2.0;
    let Some(monitor) = monitors.iter().find(|m| {
        let scale = m.scale_factor();
        let pos = m.position();
        let size = m.size();
        let x = center_x * scale;
        let y = center_y * scale;
        x >= pos.x as f64
            && x <= (pos.x + size.width as i32) as f64
            && y >= pos.y as f64
            && y <= (pos.y + size.height as i32) as f64
    }) else {
        position_assistant_fallback(app, assistant);
        return;
    };

    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    let left = work.position.x as f64 / scale;
    let top = work.position.y as f64 / scale;
    let right = left + work.size.width as f64 / scale;
    let bottom = top + work.size.height as f64 / scale;
    let min_x = left + MARGIN;
    let max_x = right - WINDOW_W - MARGIN;
    let min_y = top + MARGIN;
    let max_y = bottom - VISIBLE_PANEL_H - MARGIN;

    let x = (center_x - WINDOW_W / 2.0).clamp(min_x, max_x.max(min_x));
    let below_y = bounds.y + bounds.height + GAP;
    let above_y = bounds.y - VISIBLE_PANEL_H - GAP;
    let y = if below_y <= max_y {
        below_y
    } else if above_y >= min_y {
        above_y
    } else {
        below_y.clamp(min_y, max_y.max(min_y))
    };

    let _ = assistant.set_position(tauri::LogicalPosition::new(x, y));
}

#[cfg(target_os = "windows")]
fn position_assistant_fallback<R: Runtime>(
    app: &AppHandle<R>,
    assistant: &tauri::WebviewWindow<R>,
) {
    use crate::platform::windows::{
        PhysicalScreenRect, assistant_origin_px, logical_size_to_physical, place_window_px,
    };

    let monitor = app.primary_monitor().ok().flatten().or_else(|| {
        app.available_monitors()
            .ok()
            .and_then(|mut monitors| monitors.pop())
    });
    let Some(monitor) = monitor else {
        let _ = assistant.center();
        return;
    };
    let work = monitor.work_area();
    let Ok(width) = i32::try_from(work.size.width) else {
        return;
    };
    let Ok(height) = i32::try_from(work.size.height) else {
        return;
    };
    let work_area = PhysicalScreenRect {
        left: work.position.x,
        top: work.position.y,
        right: work.position.x.saturating_add(width),
        bottom: work.position.y.saturating_add(height),
    };
    let size = logical_size_to_physical(560.0, 136.0, monitor.scale_factor());
    let origin = assistant_origin_px(work_area, None, size, monitor.scale_factor());
    if let Ok(hwnd) = assistant.hwnd()
        && place_window_px(hwnd, origin, size, true, true).is_ok()
    {
        return;
    }
    let _ = assistant.set_position(tauri::PhysicalPosition::new(origin.x, origin.y));
}

#[cfg(not(target_os = "windows"))]
fn position_assistant_fallback<R: Runtime>(
    app: &AppHandle<R>,
    assistant: &tauri::WebviewWindow<R>,
) {
    const WINDOW_W: f64 = 560.0;
    const WINDOW_H: f64 = 136.0;
    let monitor = app
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| app.available_monitors().ok().and_then(|mut m| m.pop()));
    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let pos = monitor.position();
        let size = monitor.size();
        let x = pos.x as f64 / scale + (size.width as f64 / scale - WINDOW_W) / 2.0;
        let y = pos.y as f64 / scale + (size.height as f64 / scale - WINDOW_H) / 3.0;
        let _ = assistant.set_position(tauri::LogicalPosition::new(x, y));
    } else {
        let _ = assistant.center();
    }
}

/// 首次启动引导：640×480，5 步向导（05 §6）。
pub fn show_onboarding<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("onboarding") {
        #[cfg(target_os = "windows")]
        refresh_windows_window_icons(&w);
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    let builder = WebviewWindowBuilder::new(
        app,
        "onboarding",
        WebviewUrl::App("src/windows/onboarding/index.html".into()),
    )
    .title("")
    .inner_size(640.0, 480.0)
    .resizable(false)
    .center()
    .theme(current_native_theme(app));
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    let window = builder.build()?;
    #[cfg(target_os = "windows")]
    refresh_windows_window_icons(&window);
    #[cfg(not(target_os = "windows"))]
    let _ = &window;
    Ok(())
}
