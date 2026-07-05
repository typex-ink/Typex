//! 全部 #[tauri::command]（薄，仅转发；完整清单见 07 §10.1，按里程碑逐步补齐）。

use crate::error::{ErrorCode, TypexError};
use crate::providers::ProviderRegistry;
use crate::settings::SettingsService;
use crate::settings::schema::{Settings, SlotConfig};
use crate::settings::secrets::SecretStore;
use crate::types::{ProviderProfile, SlotKind};
use std::sync::Arc;
use tauri::State;

type SettingsState<'a> = State<'a, Arc<SettingsService>>;
type RegistryState<'a> = State<'a, Arc<ProviderRegistry>>;
type SecretsState<'a> = State<'a, Arc<dyn SecretStore>>;

#[tauri::command]
#[specta::specta]
pub fn get_settings(settings: SettingsState<'_>) -> Settings {
    settings.get()
}

#[tauri::command]
#[specta::specta]
pub fn update_settings(
    settings: SettingsState<'_>,
    new_settings: Settings,
) -> Result<Settings, TypexError> {
    settings.update(new_settings)
}

#[tauri::command]
#[specta::specta]
pub fn get_permission_status() -> Vec<crate::platform::permissions::PermissionStatus> {
    crate::platform::permissions::check_all()
}

#[tauri::command]
#[specta::specta]
pub fn open_permission_settings(kind: crate::platform::permissions::PermissionKind) {
    crate::platform::permissions::open_settings(kind);
}

#[tauri::command]
#[specta::specta]
pub fn session_command(
    commander: State<'_, crate::orchestrator::SessionCommander>,
    command: crate::orchestrator::SessionCommand,
) {
    let _ = commander.0.send(command);
}

// ── Profile 管理（07 §10.1；密钥单独走 set_profile_secret，不随 JSON 往返）──

#[tauri::command]
#[specta::specta]
pub fn list_profiles(settings: SettingsState<'_>) -> Vec<ProviderProfile> {
    settings.get().profiles
}

#[tauri::command]
#[specta::specta]
pub fn upsert_profile(
    settings: SettingsState<'_>,
    profile: ProviderProfile,
) -> Result<Settings, TypexError> {
    settings.mutate(|s| {
        s.profiles.retain(|p| p.id != profile.id);
        s.profiles.push(profile.clone());
    })
}

#[tauri::command]
#[specta::specta]
pub fn delete_profile(
    settings: SettingsState<'_>,
    secrets: SecretsState<'_>,
    profile_id: String,
) -> Result<Settings, TypexError> {
    // 删除档案时清理 keyring 凭据
    if let Some(p) = settings.get().profiles.iter().find(|p| p.id == profile_id) {
        for reference in p.credentials.values() {
            let _ = secrets.delete(reference);
        }
    }
    settings.mutate(|s| {
        s.profiles.retain(|p| p.id != profile_id);
        for slot in s.slots.values_mut() {
            if slot.active_profile.as_deref() == Some(profile_id.as_str()) {
                slot.active_profile = None;
            }
        }
    })
}

#[tauri::command]
#[specta::specta]
pub fn activate_profile(
    settings: SettingsState<'_>,
    slot: SlotKind,
    profile_id: String,
) -> Result<Settings, TypexError> {
    if !settings.get().profiles.iter().any(|p| p.id == profile_id) {
        return Err(TypexError::new(ErrorCode::InvalidRequest, "档案不存在"));
    }
    settings.mutate(|s| {
        s.slots.insert(
            slot,
            SlotConfig {
                active_profile: Some(profile_id.clone()),
            },
        );
    })
}

/// 密钥写入：field 如 "api_key" / "app_key" / "access_key"（火山双凭据，03 §6）。
#[tauri::command]
#[specta::specta]
pub fn set_profile_secret(
    settings: SettingsState<'_>,
    secrets: SecretsState<'_>,
    profile_id: String,
    field: String,
    secret: String,
) -> Result<(), TypexError> {
    let profiles = settings.get().profiles;
    let profile = profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| TypexError::new(ErrorCode::InvalidRequest, "档案不存在"))?;
    let slot_name = profile
        .slots
        .first()
        .map(|s| format!("{s:?}").to_lowercase())
        .unwrap_or_else(|| "misc".into());
    let reference = crate::settings::secrets::make_ref(&slot_name, &profile_id, &field);
    secrets.set(&reference, &secret)?;
    settings.mutate(|s| {
        if let Some(p) = s.profiles.iter_mut().find(|p| p.id == profile_id) {
            p.credentials.insert(field.clone(), reference.clone());
        }
    })?;
    Ok(())
}

// ── 助手（F-3；07 §10.1）──

/// 打字提问（语音提问走全局键）。use_selection: 是否携带当前选中文本。
#[tauri::command]
#[specta::specta]
pub fn ask_assistant(
    assistant: State<'_, Arc<crate::orchestrator::assistant::AssistantService>>,
    selection_store: State<'_, crate::app::AssistantSelection>,
    text: String,
    use_selection: bool,
) -> Result<u32, TypexError> {
    let selection = if use_selection {
        selection_store.0.lock().unwrap().clone()
    } else {
        None
    };
    assistant.ask(text, selection).map(|id| id as u32)
}

/// 面板动作：replace（模拟选区替换 = 直接注入覆盖选中）/ insert / copy。
#[tauri::command]
#[specta::specta]
pub fn assistant_action(
    injector: State<'_, Arc<crate::inject::InjectorChain>>,
    action: String,
    text: String,
) -> Result<(), TypexError> {
    match action.as_str() {
        "replace" | "insert" => {
            // 替换选区 = 焦点应用中选区仍在，注入即覆盖；插入 = 注入光标处
            injector.inject(&text)
        }
        "copy" => arboard::Clipboard::new()
            .and_then(|mut c| c.set_text(text))
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("复制失败: {e}"))),
        _ => Err(TypexError::new(ErrorCode::InvalidRequest, "未知动作")),
    }
}

/// 面板打开时读取当前选中文本（上下文芯片）。
#[tauri::command]
#[specta::specta]
pub fn read_selection_context(
    selection: State<'_, Arc<dyn crate::selection::SelectionReader>>,
    selection_store: State<'_, crate::app::AssistantSelection>,
) -> Option<String> {
    if let Some(text) = selection_store.0.lock().unwrap().clone() {
        return Some(text);
    }
    let result = selection.read().ok().flatten();
    *selection_store.0.lock().unwrap() = result.clone();
    result
}

/// 移除上下文芯片。
#[tauri::command]
#[specta::specta]
pub fn clear_selection_context(selection_store: State<'_, crate::app::AssistantSelection>) {
    *selection_store.0.lock().unwrap() = None;
}

// ── 历史（F-7；07 §10.1）──

#[tauri::command]
#[specta::specta]
pub fn query_history(
    history: State<'_, Arc<crate::history::HistoryService>>,
    search: String,
    offset: u32,
) -> Result<Vec<crate::history::HistoryItem>, TypexError> {
    history.query(&search, offset, 50)
}

#[tauri::command]
#[specta::specta]
pub fn get_stats(
    history: State<'_, Arc<crate::history::HistoryService>>,
) -> Result<crate::history::HistoryStats, TypexError> {
    history.stats()
}

#[tauri::command]
#[specta::specta]
pub fn delete_history_item(
    history: State<'_, Arc<crate::history::HistoryService>>,
    id: i32,
) -> Result<(), TypexError> {
    history.delete(id as i64)
}

#[tauri::command]
#[specta::specta]
pub fn clear_history(
    history: State<'_, Arc<crate::history::HistoryService>>,
) -> Result<(), TypexError> {
    history.clear()
}

/// 诊断报告（05 §5.2 诊断页）。
#[derive(serde::Serialize, serde::Deserialize, specta::Type, Clone)]
pub struct DiagnosticsReport {
    pub platform: String,
    pub permissions: Vec<crate::platform::permissions::PermissionStatus>,
    pub inject_backend: String,
    pub log_dir: String,
}

#[tauri::command]
#[specta::specta]
pub fn get_diagnostics(app: tauri::AppHandle) -> DiagnosticsReport {
    use tauri::Manager;
    DiagnosticsReport {
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        permissions: crate::platform::permissions::check_all(),
        inject_backend: "剪贴板粘贴（CGEvent Cmd+V）".into(),
        log_dir: app
            .path()
            .app_log_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    }
}

#[tauri::command]
#[specta::specta]
pub fn open_log_dir(app: tauri::AppHandle) {
    use tauri::Manager;
    if let Ok(dir) = app.path().app_log_dir() {
        let _ = std::process::Command::new("open").arg(dir).spawn();
    }
}

/// 打开设置窗口（主页侧边栏 ⚙）。
#[tauri::command]
#[specta::specta]
pub fn open_settings_window(app: tauri::AppHandle) -> Result<(), TypexError> {
    crate::app::windows::show_settings(&app)
        .map_err(|e| TypexError::new(ErrorCode::Internal, e.to_string()))
}

/// HUD 翻译徽标点击：在最近使用的目标语言间轮换（05 §3.2）。
#[tauri::command]
#[specta::specta]
pub fn cycle_translation_target(settings: SettingsState<'_>) -> Result<String, TypexError> {
    const DEFAULTS: [&str; 3] = ["English", "中文（简体）", "日本語"];
    let s = settings.get();
    let mut pool: Vec<String> = s.translation.recent_targets.clone();
    for d in DEFAULTS {
        if !pool.iter().any(|x| x == d) {
            pool.push(d.to_string());
        }
    }
    let cur = &s.translation.target_language;
    let idx = pool
        .iter()
        .position(|x| x == cur)
        .map(|i| (i + 1) % pool.len())
        .unwrap_or(0);
    let next = pool[idx].clone();
    let next2 = next.clone();
    settings.mutate(move |st| {
        st.translation.target_language = next2.clone();
        st.translation.recent_targets.retain(|x| x != &next2);
        st.translation.recent_targets.insert(0, next2.clone());
        st.translation.recent_targets.truncate(5);
    })?;
    Ok(next)
}

/// 测试连接（02 F-4）：STT 槽发 2 秒静音样音，LLM 槽发 ping；返回往返毫秒。
#[tauri::command]
#[specta::specta]
pub async fn test_profile(
    settings: SettingsState<'_>,
    registry: RegistryState<'_>,
    profile_id: String,
) -> Result<u32, TypexError> {
    let profiles = settings.get().profiles;
    let profile = profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| TypexError::new(ErrorCode::InvalidRequest, "档案不存在"))?;
    let start = std::time::Instant::now();
    if profile.kind.is_stt() {
        let stt = registry.build_stt(profile)?;
        // 2 秒 440Hz 正弦波样音（内容不重要，只测连通）
        let samples: Vec<f32> = (0..32000)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16000.0).sin() * 0.3)
            .collect();
        let wav = crate::audio::pipeline::to_wav_16k_mono(&samples, 16000)?;
        stt.transcribe(
            crate::providers::stt::AudioInput {
                wav_16k_mono: wav,
                duration_ms: 2000,
            },
            crate::providers::stt::SttOptions::default(),
        )
        .await
        .map_err(TypexError::from)?;
    } else {
        let llm = registry.build_llm(profile)?;
        crate::providers::llm::collect_text(
            llm.as_ref(),
            crate::providers::llm::LlmRequest {
                system: "回复 pong 一词即可".into(),
                messages: vec![crate::providers::llm::Msg {
                    role: "user".into(),
                    content: "ping".into(),
                }],
                temperature: 0.0,
                max_tokens: Some(8),
            },
        )
        .await
        .map_err(TypexError::from)?;
    }
    Ok(start.elapsed().as_millis() as u32)
}
