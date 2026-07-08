//! 应用装配与启动（main.rs 委托到此；07 §5.1 手工 DI）。

use crate::app::{PausedState, commands, events};
use crate::audio::AudioService;
use crate::hotkey::{HotkeyConfig, rdev_backend};
use crate::inject::InjectorChain;
use crate::orchestrator::Orchestrator;
use crate::providers::ProviderRegistry;
use crate::settings::SettingsService;
use futures_util::FutureExt;
use std::sync::Arc;
use tauri::Manager;
use tauri_specta::{collect_commands, collect_events};

/// tauri-specta builder：commands + events 单一注册点（gen:ipc 也用它导出 TS）。
pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::get_settings,
            commands::update_settings,
            commands::get_permission_status,
            commands::open_permission_settings,
            commands::session_command,
            commands::assistant_window_ready,
            commands::list_profiles,
            commands::upsert_profile,
            commands::delete_profile,
            commands::activate_profile,
            commands::set_profile_secret,
            commands::test_profile,
            commands::cycle_translation_target,
            commands::query_history,
            commands::get_stats,
            commands::delete_history_item,
            commands::clear_history,
            commands::open_settings_window,
            commands::open_onboarding_window,
            commands::get_diagnostics,
            commands::open_log_dir,
            commands::check_update,
            commands::install_update,
            commands::list_audio_devices,
            commands::toggle_verbatim,
            commands::export_diagnostics,
            commands::list_local_models,
            commands::get_hardware_tier,
            commands::download_local_model,
            commands::cancel_local_download,
            commands::delete_local_model,
            commands::import_local_model,
        ])
        .events(collect_events![
            events::SessionSnapshotEvent,
            events::AudioLevelEvent,
            events::SettingsChangedEvent,
            events::AssistantStartedEvent,
            events::AssistantDeltaEvent,
            events::AssistantDoneEvent,
            events::AssistantErrorEvent,
            events::UpdateAvailableEvent,
            events::LocalDownloadProgressEvent,
        ])
}

pub fn run() {
    let builder = specta_builder();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 二次启动唤起设置窗口（02 F-6）
            let _ = crate::app::windows::show_settings(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            #[cfg(target_os = "macos")]
            app.handle().plugin(tauri_nspanel::init())?;

            // 日志：dev 打终端，release 打滚动文件
            let log_dir = app.path().app_log_dir().ok();
            crate::logging::init(if cfg!(debug_assertions) {
                None
            } else {
                log_dir
            });

            // --- 服务装配（07 §5.1）---
            let config_dir = app.path().app_config_dir().expect("config dir");
            let settings = Arc::new(SettingsService::load(config_dir));
            let s = settings.get();

            let audio = Arc::new(AudioService::new());
            let injector = Arc::new(InjectorChain::platform_default(s.dictation.paste_delay_ms));

            // ProviderRegistry（CP-1.6）
            // 开发便利：TYPEX_STT_API_KEY 环境变量 → 自动建/更新 env-stt 档案
            if let Ok(key) = std::env::var("TYPEX_STT_API_KEY") {
                let base = std::env::var("TYPEX_STT_BASE_URL")
                    .unwrap_or_else(|_| "https://api.groq.com/openai/v1".into());
                let model = std::env::var("TYPEX_STT_MODEL")
                    .unwrap_or_else(|_| "whisper-large-v3-turbo".into());
                let _ = settings.mutate(|st| {
                    st.profiles.retain(|p| p.id != "env-stt");
                    st.profiles.push(crate::types::ProviderProfile {
                        id: "env-stt".into(),
                        capability: crate::types::ProviderCapability::Stt,
                        kind: crate::types::ProviderKind::OpenaiCompat,
                        label: "环境变量 STT".into(),
                        base_url: base,
                        model,
                        credentials: [("api_key".to_string(), key.trim().to_string())].into(),
                        extra_headers: Default::default(),
                        extra_form: Default::default(),
                        timeout_ms: 30_000,
                        options: Default::default(),
                    });
                    st.slots.insert(
                        crate::types::SlotKind::Stt,
                        crate::settings::schema::SlotConfig {
                            active_profile: Some("env-stt".into()),
                        },
                    );
                });
            }
            let registry = Arc::new(ProviderRegistry::new(settings.get()));
            // v1.1 本地模型（ADR-20 零配置兜底）：注入模型存储根
            #[cfg(feature = "local-models")]
            if let Ok(d) = app.path().app_data_dir() {
                registry.set_models_data_dir(d);
            }
            {
                // 设置变更 → registry 惰性失效
                let registry = registry.clone();
                let mut rx = settings.subscribe();
                tauri::async_runtime::spawn(async move {
                    while rx.changed().await.is_ok() {
                        let s = rx.borrow_and_update().clone();
                        registry.on_settings_changed(s);
                    }
                });
            }

            // 开机自启（02 F-6）：启动时对齐设置，变更时跟随开关
            {
                use tauri_plugin_autostart::ManagerExt;
                let apply = |handle: &tauri::AppHandle, on: bool| {
                    let mgr = handle.autolaunch();
                    let r = if on { mgr.enable() } else { mgr.disable() };
                    if let Err(e) = r {
                        tracing::warn!("开机自启设置失败: {e}");
                    }
                };
                apply(app.handle(), s.general.autostart);
                let handle = app.handle().clone();
                let mut rx = settings.subscribe();
                tauri::async_runtime::spawn(async move {
                    let mut last = None;
                    while rx.changed().await.is_ok() {
                        let on = rx.borrow_and_update().general.autostart;
                        if last != Some(on) {
                            last = Some(on);
                            apply(&handle, on);
                        }
                    }
                });
            }

            // 自动更新（ADR-11：检查自动、安装需确认）——启动 10s 后后台检查一次
            if s.general.check_updates && !cfg!(debug_assertions) {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    use tauri_plugin_updater::UpdaterExt;
                    use tauri_specta::Event;
                    let Ok(updater) = handle.updater() else {
                        return;
                    };
                    if let Ok(Some(u)) = updater.check().await {
                        tracing::info!("发现新版本: {}", u.version);
                        let _ = crate::app::events::UpdateAvailableEvent {
                            version: u.version.clone(),
                            notes: u.body.clone().unwrap_or_default(),
                        }
                        .emit(&handle);
                    }
                });
            }

            // 暂停状态（托盘切换）
            let (paused_tx, paused_rx) = tokio::sync::watch::channel(false);
            app.manage(PausedState(paused_tx));
            // 本地模型下载任务表（v1.1；默认构建恒空）
            app.manage(crate::app::LocalDownloads::default());
            let assistant_window_ready =
                Arc::new(crate::app::commands::AssistantWindowReady::default());
            app.manage(assistant_window_ready.clone());

            // hotkey 线程（配置热更新：settings watch → HotkeyConfig watch 桥接）
            let hotkey_cfg = HotkeyConfig::from_settings(&s.hotkeys);
            let (cfg_tx, cfg_rx) = tokio::sync::watch::channel(hotkey_cfg.clone());
            {
                let mut settings_rx = settings.subscribe();
                tauri::async_runtime::spawn(async move {
                    while settings_rx.changed().await.is_ok() {
                        let cfg = HotkeyConfig::from_settings(&settings_rx.borrow().hotkeys);
                        let _ = cfg_tx.send(cfg);
                    }
                });
            }
            let hotkey_rx = rdev_backend::spawn(hotkey_cfg, cfg_rx, paused_rx);

            // orchestrator 主循环：快照/电平经 IPC event 推给前端
            let last_result = Arc::new(std::sync::Mutex::new(None::<String>));
            let pending_selection = Arc::new(std::sync::Mutex::new(None::<String>));
            let selection: Arc<dyn crate::selection::SelectionReader> =
                Arc::from(crate::selection::platform_default());

            // AssistantService（F-3 / ADR-23）：流式事件 → assistant:// IPC events；
            // 回答型确认时经 show_panel 回调呼出回答弹窗
            let handle_a = app.handle().clone();
            let handle_panel = app.handle().clone();
            let ready_panel = assistant_window_ready.clone();
            let assistant = Arc::new(crate::orchestrator::assistant::AssistantService::new(
                settings.clone(),
                registry.clone(),
                Box::new(move |ev| {
                    use crate::orchestrator::assistant::AssistantEvent;
                    use tauri_specta::Event as _;
                    match ev {
                        AssistantEvent::Started {
                            request_id,
                            instruction,
                            selection_chars,
                            degraded,
                        } => {
                            let _ = events::AssistantStartedEvent {
                                request_id: request_id as u32,
                                instruction,
                                selection_chars,
                                degraded,
                            }
                            .emit(&handle_a);
                        }
                        AssistantEvent::Delta { request_id, text } => {
                            let _ = events::AssistantDeltaEvent {
                                request_id: request_id as u32,
                                text_delta: text,
                            }
                            .emit(&handle_a);
                        }
                        AssistantEvent::Done {
                            request_id,
                            full_text,
                        } => {
                            let _ = events::AssistantDoneEvent {
                                request_id: request_id as u32,
                                full_text,
                            }
                            .emit(&handle_a);
                        }
                        AssistantEvent::Error { request_id, error } => {
                            let _ = events::AssistantErrorEvent {
                                request_id: request_id as u32,
                                error,
                            }
                            .emit(&handle_a);
                        }
                    }
                }),
                Box::new(move |has_selection| {
                    let handle_panel = handle_panel.clone();
                    let ready_panel = ready_panel.clone();
                    async move {
                        let is_new_window = handle_panel.get_webview_window("assistant").is_none();
                        if is_new_window {
                            ready_panel.reset();
                        }
                        let _ = crate::app::windows::show_assistant(&handle_panel, has_selection);
                        if is_new_window || !ready_panel.is_ready() {
                            let _ = ready_panel
                                .wait_ready(std::time::Duration::from_millis(1500))
                                .await;
                        }
                    }
                    .boxed()
                }),
            ));

            // 历史记录（F-7）：data dir + 启动保留期清理
            let history = {
                let data_dir = app.path().app_data_dir().expect("data dir");
                match crate::history::HistoryService::open(&data_dir.join("history.sqlite")) {
                    Ok(h) => {
                        let h = Arc::new(h);
                        let retention = settings.get().history.retention_days;
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        if let Ok(n) = h.cleanup(retention, now)
                            && n > 0
                        {
                            tracing::info!("历史保留期清理: 删除 {n} 条");
                        }
                        Some(h)
                    }
                    Err(e) => {
                        tracing::error!("历史库打开失败: {}", e.message);
                        None
                    }
                }
            };

            let handle = app.handle().clone();
            let handle2 = app.handle().clone();
            // 托盘视觉状态（CP-6.9：snapshot/电平 → 图标动画）
            let tray_visual = Arc::new(crate::app::tray_icon::TrayVisual::default());
            app.manage(tray_visual.clone());
            {
                // 暂停开关 → 托盘图标 40% 透明 + 斜杠
                let tv = tray_visual.clone();
                let mut rx = app.state::<PausedState>().0.subscribe();
                tauri::async_runtime::spawn(async move {
                    while rx.changed().await.is_ok() {
                        tv.set_paused(*rx.borrow_and_update());
                    }
                });
            }
            let tv_snap = tray_visual.clone();
            let tv_level = tray_visual.clone();
            let orch = Arc::new(Orchestrator {
                settings: settings.clone(),
                audio,
                injector: injector.clone(),
                registry: registry.clone(),
                snapshot_sink: Box::new(move |snap| {
                    use tauri_specta::Event as _;
                    crate::app::windows::sync_hud_visibility(&handle, &snap);
                    crate::app::tray::update_status(&handle, &snap);
                    tv_snap.on_snapshot(&snap);
                    let _ = events::SessionSnapshotEvent(snap).emit(&handle);
                }),
                level_sink: Box::new(move |levels| {
                    use tauri_specta::Event as _;
                    tv_level.on_levels(&levels);
                    let _ = events::AudioLevelEvent(levels).emit(&handle2);
                }),
                last_result: last_result.clone(),
                assistant: Some(assistant.clone()),
                pending_selection: pending_selection.clone(),
                selection_read_failed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                selection: selection.clone(),
                history: history.clone(),
            });
            let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
            app.manage(crate::orchestrator::SessionCommander(cmd_tx));
            app.manage(crate::app::LastResult(last_result.clone()));
            app.manage(assistant);
            app.manage(selection);
            app.manage(injector);
            if let Some(h) = &history {
                app.manage(h.clone());
            }
            tauri::async_runtime::spawn(orch.run(hotkey_rx, cmd_rx));

            let settings_for_onboarding = settings.clone();
            app.manage(settings);
            app.manage(registry);

            // 托盘
            crate::app::tray::setup(app.handle())?;
            crate::app::tray_icon::spawn_animator(app.handle().clone());
            {
                // 设置变更（含设置窗口改动）→ 托盘菜单重建 + 全窗口广播
                let handle = app.handle().clone();
                let mut rx = settings_for_onboarding.subscribe();
                tauri::async_runtime::spawn(async move {
                    while rx.changed().await.is_ok() {
                        let s = rx.borrow_and_update().clone();
                        crate::app::tray::refresh(&handle);
                        use tauri_specta::Event as _;
                        let _ = events::SettingsChangedEvent(s).emit(&handle);
                    }
                });
            }

            // HUD → nonactivating NSPanel（07 §7.2 坑 3：抢焦点会毁掉注入）
            #[cfg(target_os = "macos")]
            crate::app::windows::setup_hud_panel(app.handle())?;

            // macOS：显示在 Dock（Regular 默认即是；点击 Dock 图标打开主页在 RunEvent::Reopen 处理）

            // 首次启动 → 引导向导（02 F-8）
            if !settings_for_onboarding.get().onboarding_done {
                crate::app::windows::show_onboarding(app.handle())?;
            }

            // 开发调试：TYPEX_OPEN=home|settings|assistant 直接打开窗口
            if cfg!(debug_assertions) {
                match std::env::var("TYPEX_OPEN").as_deref() {
                    Ok("home") => crate::app::windows::show_home(app.handle())?,
                    Ok("settings") => crate::app::windows::show_settings(app.handle())?,
                    Ok("assistant") => crate::app::windows::show_assistant(app.handle(), false)?,
                    _ => {}
                }
            }

            tracing::info!("Typex 启动完成");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run({
            // 启动激活也会触发一次 Reopen——宽限期内忽略，保持「启动后无窗口」（05 §2）
            let launched_at = std::time::Instant::now();
            move |app, event| {
                // 点击 Dock 图标（无可见窗口时）→ 打开主页（05 §8：Dock/托盘按需打开）
                #[cfg(target_os = "macos")]
                if let tauri::RunEvent::Reopen {
                    has_visible_windows,
                    ..
                } = event
                    && !has_visible_windows
                    && launched_at.elapsed().as_secs() >= 2
                {
                    let _ = crate::app::windows::show_home(app);
                }
                #[cfg(not(target_os = "macos"))]
                let _ = (app, event, launched_at);
            }
        });
}

/// IPC bindings 导出（`pnpm gen:ipc` 触发；CI 校验新鲜度）。
#[cfg(test)]
mod export {
    #[test]
    fn export_bindings() {
        super::specta_builder()
            .export(
                specta_typescript::Typescript::default()
                    // u64 全部是 ms/字节等小数值，安全映射为 number
                    .bigint(specta_typescript::BigIntExportBehavior::Number)
                    .header("// @ts-nocheck\n// 由 tauri-specta 生成 — 禁止手改（pnpm gen:ipc 重新生成）\n"),
                "../src/ipc/bindings.ts",
            )
            .expect("导出 TS bindings 失败");
    }
}
