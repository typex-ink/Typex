//! 托盘图标 + 完整菜单（02 F-6 / 05 §2）。
//! 状态行随会话变化；翻译目标/模型子菜单动态生成；勾选项即时切换。

use crate::settings::SettingsService;
use crate::types::{SessionPhase, SessionSnapshot, SlotKind};
use std::sync::Arc;
use tauri::{
    AppHandle, Manager, Runtime,
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const TARGET_LANGS: [&str; 6] = [
    "English",
    "中文（简体）",
    "日本語",
    "한국어",
    "Français",
    "Deutsch",
];

/// 构建整套菜单（配置/状态变化时重建——菜单结构简单，重建成本可忽略）。
fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    settings: &SettingsService,
    status_text: &str,
    paused: bool,
) -> tauri::Result<tauri::menu::Menu<R>> {
    let s = settings.get();

    let status = MenuItemBuilder::with_id("status", status_text)
        .enabled(false)
        .build(app)?;
    let copy_last = MenuItemBuilder::with_id("copy_last", "复制上次结果").build(app)?;

    let polish = CheckMenuItemBuilder::with_id("polish", "文本整理")
        .checked(s.dictation.polish_enabled)
        .build(app)?;

    // 翻译目标 ▸ 子菜单（05 §2）
    let mut target_menu = SubmenuBuilder::with_id(app, "target_menu", "翻译目标");
    for lang in TARGET_LANGS {
        let item = CheckMenuItemBuilder::with_id(format!("target:{lang}"), lang)
            .checked(s.translation.target_language == lang)
            .build(app)?;
        target_menu = target_menu.item(&item);
    }
    let target_menu = target_menu.build()?;

    // 模型 ▸ 子菜单（ADR-21：听写/翻译/问答 三组档案快速切换）
    let mut model_menu = SubmenuBuilder::with_id(app, "model_menu", "模型");
    let groups: [(&str, SlotKind); 3] = [
        ("听写", SlotKind::Stt),
        ("翻译", SlotKind::Translate),
        ("问答", SlotKind::Assistant),
    ];
    let mut first_group = true;
    for (label, slot) in groups {
        let profiles: Vec<_> = s
            .profiles
            .iter()
            .filter(|p| p.capability == slot.capability())
            .collect();
        if profiles.is_empty() {
            continue;
        }
        if !first_group {
            model_menu = model_menu.separator();
        }
        first_group = false;
        let header = MenuItemBuilder::with_id(format!("mh:{label}"), label)
            .enabled(false)
            .build(app)?;
        model_menu = model_menu.item(&header);
        let active = s.slots.get(&slot).and_then(|c| c.active_profile.as_deref());
        for p in profiles {
            let item =
                CheckMenuItemBuilder::with_id(format!("model:{:?}:{}", slot, p.id), &p.label)
                    .checked(active == Some(p.id.as_str()))
                    .build(app)?;
            model_menu = model_menu.item(&item);
        }
    }
    let model_menu = model_menu.build()?;

    let pause = MenuItemBuilder::with_id(
        "pause",
        if paused {
            "恢复 Typex"
        } else {
            "暂停 Typex"
        },
    )
    .build(app)?;
    let settings_item = MenuItemBuilder::with_id("settings", "设置…")
        .accelerator("Cmd+,")
        .build(app)?;
    let home = MenuItemBuilder::with_id("home", "主页…").build(app)?;
    let update = MenuItemBuilder::with_id("update", "检查更新").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    MenuBuilder::new(app)
        .item(&status)
        .separator()
        .item(&copy_last)
        .separator()
        .item(&polish)
        .item(&target_menu)
        .item(&model_menu)
        .item(&pause)
        .separator()
        .item(&settings_item)
        .item(&home)
        .item(&update)
        .item(&quit)
        .build()
}

/// 状态行文本（05 §2）。
pub fn status_line(snap: Option<&SessionSnapshot>, paused: bool) -> String {
    if paused {
        return "◌ Typex 已暂停".into();
    }
    match snap.map(|s| s.phase) {
        Some(SessionPhase::Recording) => "● 录音中".into(),
        Some(SessionPhase::Transcribing)
        | Some(SessionPhase::Processing)
        | Some(SessionPhase::Injecting) => "◐ 处理中".into(),
        Some(SessionPhase::Failed) => "⚠ 上次会话失败".into(),
        _ => "● Typex 就绪".into(),
    }
}

pub fn setup<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let settings = app.state::<Arc<SettingsService>>().inner().clone();
    let menu = build_menu(app, &settings, &status_line(None, false), false)?;

    TrayIconBuilder::with_id("main")
        .icon(
            tauri::image::Image::from_bytes(include_bytes!("../../icons/tray.png"))
                .expect("tray icon"),
        )
        .icon_as_template(true)
        .tooltip("Typex")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_tray_icon_event(|tray, event| {
            if cfg!(target_os = "macos") {
                return;
            }
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = crate::app::windows::show_home(tray.app_handle());
            }
        })
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            match id {
                "quit" => app.exit(0),
                "pause" => {
                    if let Some(paused) = app.try_state::<crate::app::PausedState>() {
                        let cur = *paused.0.borrow();
                        let _ = paused.0.send(!cur);
                        refresh(app);
                    }
                }
                "settings" => {
                    let _ = crate::app::windows::show_settings(app);
                }
                "home" => {
                    let _ = crate::app::windows::show_home(app);
                }
                "copy_last" => {
                    if let Some(last) = app.try_state::<crate::app::LastResult>() {
                        let text = last.0.lock().unwrap().clone();
                        if let Some(text) = text {
                            let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(text));
                        }
                    }
                }
                "polish" => {
                    let settings = app.state::<Arc<SettingsService>>();
                    let _ = settings
                        .mutate(|s| s.dictation.polish_enabled = !s.dictation.polish_enabled);
                    refresh(app);
                }
                "update" => {
                    // 手动检查（ADR-11：安装需确认）：有新版本发事件 + 打开设置-关于页确认
                    let handle = app.clone();
                    let settings = app.state::<Arc<SettingsService>>().inner().clone();
                    tauri::async_runtime::spawn(async move {
                        use tauri_specta::Event;
                        let channel = settings.get().general.update_channel;
                        match crate::app::update::check(&handle, channel).await {
                            Ok(Some(u)) => {
                                let _ = crate::app::events::UpdateAvailableEvent {
                                    version: u.version.clone(),
                                    notes: u.body.clone().unwrap_or_default(),
                                }
                                .emit(&handle);
                                let _ = crate::app::windows::show_settings(&handle);
                            }
                            Ok(None) => tracing::info!("检查更新：已是最新版本"),
                            Err(e) => tracing::warn!("检查更新失败: {e}"),
                        }
                    });
                }
                id if id.starts_with("target:") => {
                    let lang = id.trim_start_matches("target:").to_string();
                    let settings = app.state::<Arc<SettingsService>>();
                    let _ = settings.mutate(|s| s.translation.target_language = lang);
                    refresh(app);
                }
                id if id.starts_with("model:") => {
                    // model:<SlotDebug>:<profile-id>
                    let rest = id.trim_start_matches("model:");
                    if let Some((slot_str, profile_id)) = rest.split_once(':') {
                        let slot = match slot_str {
                            "Stt" => Some(SlotKind::Stt),
                            "Polish" => Some(SlotKind::Polish),
                            "Translate" => Some(SlotKind::Translate),
                            "Assistant" => Some(SlotKind::Assistant),
                            _ => None,
                        };
                        if let Some(slot) = slot {
                            let settings = app.state::<Arc<SettingsService>>();
                            let pid = profile_id.to_string();
                            let _ = settings.mutate(|s| {
                                s.slots.insert(
                                    slot,
                                    crate::settings::schema::SlotConfig {
                                        active_profile: Some(pid.clone()),
                                    },
                                );
                            });
                            refresh(app);
                        }
                    }
                }
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}

/// 重建菜单（设置变更/状态变化后）。
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let settings = app.state::<Arc<SettingsService>>().inner().clone();
    let paused = app
        .try_state::<crate::app::PausedState>()
        .map(|p| *p.0.borrow())
        .unwrap_or(false);
    if let Ok(menu) = build_menu(app, &settings, &status_line(None, paused), paused) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// 会话状态变化 → 状态行更新（orchestrator snapshot_sink 调用）。
pub fn update_status<R: Runtime>(app: &AppHandle<R>, snap: &SessionSnapshot) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let settings = app.state::<Arc<SettingsService>>().inner().clone();
    let paused = app
        .try_state::<crate::app::PausedState>()
        .map(|p| *p.0.borrow())
        .unwrap_or(false);
    if let Ok(menu) = build_menu(app, &settings, &status_line(Some(snap), paused), paused) {
        let _ = tray.set_menu(Some(menu));
    }
}
