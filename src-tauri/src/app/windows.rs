//! 窗口创建/显隐/定位（06 §2）。HUD 使用 nonactivating NSPanel。

use crate::selection::{SelectionBounds, SelectionReader};
use crate::types::{SessionPhase, SessionSnapshot};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// HUD 显隐代际：每次快照 +1；延迟隐藏只在代际未变时执行（防止杀掉新会话的 HUD）。
static HUD_GEN: AtomicU64 = AtomicU64::new(0);

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
            if !hud.is_visible().unwrap_or(false) {
                position_hud(&hud);
                let _ = hud.show();
            }
        }
    }
}

/// HUD 位置：屏幕底部居中，距底边 48px（05 §3.1）。
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
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        "settings",
        WebviewUrl::App("src/windows/settings/index.html".into()),
    )
    .title("")
    .inner_size(720.0, 520.0)
    .resizable(false)
    .build()?;
    Ok(())
}

/// 主页窗口：880×560 侧边栏导航（05 §8 / ADR-19）。
pub fn show_home<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("home") {
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        "home",
        WebviewUrl::App("src/windows/home/index.html".into()),
    )
    .title("")
    .inner_size(880.0, 560.0)
    .resizable(false)
    .center()
    .build()?;
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
    .visible(false)
    .build()?;
    position_assistant(app, &w, selection_bounds);
    w.show()?;
    w.set_focus()?;
    Ok(())
}

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
    .center();
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder.build()?;
    Ok(())
}
