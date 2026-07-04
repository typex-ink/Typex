//! 窗口创建/显隐/定位（07 §2）。HUD NSPanel 处理在 CP-1.3。

use crate::types::{SessionPhase, SessionSnapshot};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// HUD 显隐代际：每次快照 +1；延迟隐藏只在代际未变时执行（防止杀掉新会话的 HUD）。
static HUD_GEN: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    // HUD：nonactivating NSPanel——显示时绝不抢焦点（07 §7.2 坑 3）
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
    let Some(hud) = app.get_webview_window("hud") else { return Ok(()) };
    let panel = hud.to_panel::<HudPanel>()?;
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
    panel.set_level(PanelLevel::ScreenSaver.into()); // 全屏应用之上也可见
    panel.set_collection_behavior(
        CollectionBehavior::new().can_join_all_spaces().full_screen_auxiliary().value(),
    );
    Ok(())
}

/// HUD 显隐随会话状态（05 §3.3）。Idle 延迟 700ms 隐藏——给前端 600ms 成功反馈留时间；
/// 期间若新会话开始，show 会先到，隐藏检查会话仍为 Idle 才执行。
pub fn sync_hud_visibility<R: Runtime>(app: &AppHandle<R>, snap: &SessionSnapshot) {
    let Some(hud) = app.get_webview_window("hud") else { return };
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
        let hud_size = hud.outer_size().unwrap_or(tauri::PhysicalSize::new(320, 44));
        let x = (screen.width as i32 - hud_size.width as i32) / 2;
        let y = screen.height as i32 - hud_size.height as i32 - (48.0 * scale) as i32;
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
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("src/windows/settings/index.html".into()))
        .title("Typex 设置")
        .inner_size(720.0, 520.0)
        .resizable(false)
        .build()?;
    Ok(())
}
