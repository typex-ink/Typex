//! 全部 #[tauri::command]（薄，仅转发；完整清单见 07 §10.1，按里程碑逐步补齐）。

use crate::error::{ErrorCode, TypexError};
use crate::providers::ProviderRegistry;
use crate::settings::SettingsService;
use crate::settings::schema::{Settings, SlotConfig};
use crate::settings::secrets::SecretStore;
use crate::types::{ProviderCapability, ProviderKind, ProviderProfile, SlotKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::State;
use tokio::sync::Notify;

type SettingsState<'a> = State<'a, Arc<SettingsService>>;
type RegistryState<'a> = State<'a, Arc<ProviderRegistry>>;
type SecretsState<'a> = State<'a, Arc<dyn SecretStore>>;
type AssistantReadyState<'a> = State<'a, Arc<AssistantWindowReady>>;

pub struct AssistantWindowReady {
    ready: AtomicBool,
    notify: Notify,
}

impl Default for AssistantWindowReady {
    fn default() -> Self {
        Self {
            ready: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }
}

impl AssistantWindowReady {
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    pub fn reset(&self) {
        self.ready.store(false, Ordering::Release);
    }

    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub async fn wait_ready(&self, timeout: std::time::Duration) -> bool {
        if self.is_ready() {
            return true;
        }
        let notified = self.notify.notified();
        if self.is_ready() {
            return true;
        }
        tokio::time::timeout(timeout, notified).await.is_ok() || self.is_ready()
    }
}

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

/// 助手窗口前端已注册 assistant:// 事件监听器。
#[tauri::command]
#[specta::specta]
pub fn assistant_window_ready(ready: AssistantReadyState<'_>) {
    ready.mark_ready();
}

// ── Profile 管理（07 §10.1；密钥单独走 set_profile_secret，不随 JSON 往返）──

#[tauri::command]
#[specta::specta]
pub fn list_profiles(settings: SettingsState<'_>) -> Vec<ProviderProfile> {
    settings.get().profiles
}

fn capability_name(capability: ProviderCapability) -> &'static str {
    match capability {
        ProviderCapability::Stt => "stt",
        ProviderCapability::Llm => "llm",
    }
}

fn profile_kind_matches_capability(profile: &ProviderProfile) -> bool {
    match profile.capability {
        ProviderCapability::Stt => matches!(
            profile.kind,
            ProviderKind::OpenaiCompat | ProviderKind::Volcengine | ProviderKind::Local
        ),
        ProviderCapability::Llm => matches!(
            profile.kind,
            ProviderKind::ChatCompletions | ProviderKind::Responses | ProviderKind::Local
        ),
    }
}

fn ensure_profile_compatible(slot: SlotKind, profile: &ProviderProfile) -> Result<(), TypexError> {
    if profile.capability != slot.capability() {
        return Err(TypexError::new(
            ErrorCode::InvalidRequest,
            format!("档案 {} 不能用于 {slot:?} 槽", profile.id),
        ));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn upsert_profile(
    settings: SettingsState<'_>,
    profile: ProviderProfile,
) -> Result<Settings, TypexError> {
    if !profile_kind_matches_capability(&profile) {
        return Err(TypexError::new(
            ErrorCode::InvalidRequest,
            format!("{:?} 与 {:?} 不兼容", profile.kind, profile.capability),
        ));
    }
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
    let current = settings.get();
    let profile = current
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| TypexError::new(ErrorCode::InvalidRequest, "档案不存在"))?;
    ensure_profile_compatible(slot, profile)?;
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
    let reference = profile.credentials.get(&field).cloned().unwrap_or_else(|| {
        crate::settings::secrets::make_ref(capability_name(profile.capability), &profile_id, &field)
    });
    secrets.set(&reference, &secret)?;
    settings.mutate(|s| {
        if let Some(p) = s.profiles.iter_mut().find(|p| p.id == profile_id) {
            p.credentials.insert(field.clone(), reference.clone());
        }
    })?;
    Ok(())
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
    /// 硬件信息摘要（仅 feature = local-models 时填充；默认构建为 None）。
    /// 格式示例：`RAM 24 GB · 10 核 · Metal ✓ · 推荐档位：性能`
    pub hardware: Option<String>,
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
        #[cfg(feature = "local-models")]
        hardware: Some(crate::local::hardware::diagnostics_string()),
        #[cfg(not(feature = "local-models"))]
        hardware: None,
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

/// 打开首次启动引导（设置 → 调试）。
#[tauri::command]
#[specta::specta]
pub fn open_onboarding_window(app: tauri::AppHandle) -> Result<(), TypexError> {
    crate::app::windows::show_onboarding(&app)
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
    let treat_as_stt = profile.capability == ProviderCapability::Stt;
    if treat_as_stt {
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

/// 更新检查结果（CP-6.3 / ADR-11）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
}

/// 检查更新：有新版本返回 Some（不下载）；安装需用户确认后调 install_update。
#[tauri::command]
#[specta::specta]
pub async fn check_update(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, TypexError> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app
        .updater()
        .map_err(|e| TypexError::new(ErrorCode::NotConfigured, format!("updater 未配置: {e}")))?;
    match updater.check().await {
        Ok(Some(u)) => Ok(Some(UpdateInfo {
            version: u.version.clone(),
            notes: u.body.clone().unwrap_or_default(),
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(TypexError::new(
            ErrorCode::NetworkError,
            format!("检查更新失败: {e}"),
        )),
    }
}

/// 下载并安装更新（用户已确认，ADR-11：安装需确认）；成功后重启应用。
#[tauri::command]
#[specta::specta]
pub async fn install_update(app: tauri::AppHandle) -> Result<(), TypexError> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app
        .updater()
        .map_err(|e| TypexError::new(ErrorCode::NotConfigured, format!("updater 未配置: {e}")))?;
    let update = updater
        .check()
        .await
        .map_err(|e| TypexError::new(ErrorCode::NetworkError, format!("检查更新失败: {e}")))?
        .ok_or_else(|| TypexError::new(ErrorCode::InvalidRequest, "当前已是最新版本"))?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| TypexError::new(ErrorCode::NetworkError, format!("下载安装失败: {e}")))?;
    app.restart();
}

/// 枚举输入设备（听写页麦克风下拉，CP-6.4）。
#[tauri::command]
#[specta::specta]
pub fn list_audio_devices() -> Vec<String> {
    crate::audio::list_input_devices()
}

/// HUD 一键切换原样模式（02 F-9：HUD 与设置均可切换）；返回切换后 verbatim 状态。
#[tauri::command]
#[specta::specta]
pub fn toggle_verbatim(settings: SettingsState<'_>) -> Result<bool, TypexError> {
    let mut verbatim = false;
    settings.mutate(|s| {
        s.dictation.polish_enabled = !s.dictation.polish_enabled;
        verbatim = !s.dictation.polish_enabled;
    })?;
    Ok(verbatim)
}

/// 导出诊断包（05 §5.2 / CP-6.11）：环境自检 + 脱敏 settings + 最近日志 → zip 到下载目录；
/// 返回生成的文件路径。密钥引用与凭据字段一律剔除。
#[tauri::command]
#[specta::specta]
pub fn export_diagnostics(
    app: tauri::AppHandle,
    settings: SettingsState<'_>,
) -> Result<String, TypexError> {
    use std::io::Write;
    use tauri::Manager;

    let dest_dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|e| TypexError::new(ErrorCode::Internal, format!("找不到导出目录: {e}")))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest = dest_dir.join(format!("typex-diagnostics-{stamp}.zip"));
    let file = std::fs::File::create(&dest)
        .map_err(|e| TypexError::new(ErrorCode::Internal, format!("创建诊断包失败: {e}")))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();

    // 1. 环境自检报告
    let report = get_diagnostics(app.clone());
    zip.start_file("diagnostics.json", opts)
        .and_then(|_| {
            zip.write_all(
                serde_json::to_string_pretty(&report)
                    .unwrap_or_default()
                    .as_bytes(),
            )
            .map_err(zip::result::ZipError::Io)
        })
        .map_err(|e| TypexError::new(ErrorCode::Internal, format!("写诊断包失败: {e}")))?;

    // 2. 脱敏 settings：credentials 全剔除（keyring 引用路径含 profile 结构也不导出）
    let mut s = settings.get();
    for p in &mut s.profiles {
        p.credentials.clear();
    }
    zip.start_file("settings.redacted.json", opts)
        .and_then(|_| {
            zip.write_all(
                serde_json::to_string_pretty(&s)
                    .unwrap_or_default()
                    .as_bytes(),
            )
            .map_err(zip::result::ZipError::Io)
        })
        .map_err(|e| TypexError::new(ErrorCode::Internal, format!("写诊断包失败: {e}")))?;

    // 3. 最近日志（写入层已 redact；此处再过一遍以防旧日志）
    if let Ok(log_dir) = app.path().app_log_dir()
        && let Ok(entries) = std::fs::read_dir(&log_dir)
    {
        let mut logs: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("typex.log"))
            .collect();
        logs.sort_by_key(|e| e.file_name());
        // 只带最近 3 个滚动文件
        for entry in logs.iter().rev().take(3) {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                let redacted: String = content
                    .lines()
                    .map(|l| crate::logging::redact(l) + "\n")
                    .collect();
                let name = format!("logs/{}", entry.file_name().to_string_lossy());
                let _ = zip.start_file(name, opts).and_then(|_| {
                    zip.write_all(redacted.as_bytes())
                        .map_err(zip::result::ZipError::Io)
                });
            }
        }
    }

    zip.finish()
        .map_err(|e| TypexError::new(ErrorCode::Internal, format!("封包失败: {e}")))?;
    Ok(dest.display().to_string())
}

// ── 本地模型（v1.1 / F-12 / ADR-20/22；CP-8.7/8.8）─────────────────────────────
//
// IPC 契约（类型 + command 签名）无条件定义——collect_commands! 不能按 feature
// 条件包含单项；实现在函数体内 #[cfg(feature = "local-models")] 分支，
// 默认构建返回 NotConfigured「本地模型未启用」或空值。

/// 模型库条目 + 本机状态（list_local_models 载荷）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LocalModelInfo {
    /// 模型库 id（= 存储子目录名 = local 档案的 model 字段）。
    pub id: String,
    pub display_name: String,
    /// "stt" | "llm"
    pub purpose: String,
    /// "sherpa" | "llama"
    pub engine: String,
    /// 全部文件合计字节。
    pub bytes: u64,
    pub downloaded: bool,
    /// 正在下载中（有进行中的下载任务）。
    pub downloading: bool,
    /// 最低推荐 RAM（GiB）。
    pub min_ram_gb: u32,
    pub requires_gpu: bool,
    /// 本机是否满足硬件要求（RAM ≥ min_ram_gb 且（不需 GPU 或有 GPU））。
    pub hardware_ok: bool,
    /// 所属推荐档位 key："light" | "standard" | "performance"。
    pub tier: String,
}

/// 硬件探测结果（get_hardware_tier 载荷）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct HardwareTier {
    pub ram_gb: u32,
    pub cores: u32,
    /// GPU 加速可用（macOS = Metal）。
    pub gpu: bool,
    /// 推荐档位 key："light" | "standard" | "performance"。
    pub tier: String,
    /// 诊断页格式的摘要串，如 `RAM 24 GB · 10 核 · Metal ✓ · 推荐档位：性能`。
    pub summary: String,
}

#[cfg(feature = "local-models")]
fn local_models_data_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, TypexError> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .map_err(|e| TypexError::new(ErrorCode::Internal, format!("找不到数据目录: {e}")))
}

/// 模型库全量 + 每条的下载/硬件状态（默认构建返回空列表）。
#[tauri::command]
#[specta::specta]
pub fn list_local_models(
    app: tauri::AppHandle,
    downloads: State<'_, crate::app::LocalDownloads>,
) -> Result<Vec<LocalModelInfo>, TypexError> {
    #[cfg(feature = "local-models")]
    {
        use crate::local::{download, hardware, manifest};
        let data_dir = local_models_data_dir(&app)?;
        let catalog = manifest::catalog();
        let downloaded = download::list_downloaded(&data_dir, &catalog);
        let hw = hardware::detect();
        let active = downloads.0.lock().unwrap();
        Ok(catalog
            .into_iter()
            .map(|e| LocalModelInfo {
                downloaded: downloaded.contains(&e.id),
                downloading: active.get(&e.id).is_some_and(|h| !h.inner().is_finished()),
                purpose: match e.purpose {
                    manifest::ModelPurpose::Stt => "stt".into(),
                    manifest::ModelPurpose::Llm => "llm".into(),
                },
                engine: match e.engine {
                    manifest::ModelEngine::Sherpa => "sherpa".into(),
                    manifest::ModelEngine::Llama => "llama".into(),
                },
                bytes: e.files.iter().map(|f| f.bytes).sum(),
                hardware_ok: hw.ram_gb >= e.min_ram_gb as u64
                    && (!e.requires_gpu || hw.gpu_available),
                tier: hardware::tier_of_model(&e.id)
                    .map(|t| t.key().to_string())
                    .unwrap_or_default(),
                min_ram_gb: e.min_ram_gb,
                requires_gpu: e.requires_gpu,
                id: e.id,
                display_name: e.display_name,
            })
            .collect())
    }
    #[cfg(not(feature = "local-models"))]
    {
        let _ = (app, downloads);
        Ok(Vec::new())
    }
}

/// 硬件探测 + 推荐档位（默认构建返回 None）。
#[tauri::command]
#[specta::specta]
pub fn get_hardware_tier() -> Option<HardwareTier> {
    #[cfg(feature = "local-models")]
    {
        use crate::local::hardware;
        let hw = hardware::detect();
        let tier = hardware::recommend_tier(hw.ram_gb, hw.cpu_cores, hw.gpu_available);
        Some(HardwareTier {
            ram_gb: hw.ram_gb as u32,
            cores: hw.cpu_cores as u32,
            gpu: hw.gpu_available,
            tier: tier.key().into(),
            summary: hardware::diagnostics_string(),
        })
    }
    #[cfg(not(feature = "local-models"))]
    {
        None
    }
}

/// 启动模型下载（tokio task 后台跑；进度经 `local://download-progress` 推送）。
/// 已在下载中 / 已下载 → 幂等返回 Ok。
#[tauri::command]
#[specta::specta]
pub fn download_local_model(
    app: tauri::AppHandle,
    downloads: State<'_, crate::app::LocalDownloads>,
    settings: SettingsState<'_>,
    model_id: String,
    source: Option<crate::types::ModelDownloadSource>,
) -> Result<(), TypexError> {
    #[cfg(feature = "local-models")]
    {
        use crate::local::{download, manifest};
        use tauri_specta::Event as _;
        let data_dir = local_models_data_dir(&app)?;
        let source = source.unwrap_or_else(|| settings.get().general.model_download_source);
        let entry = manifest::catalog()
            .into_iter()
            .find(|m| m.id == model_id)
            .ok_or_else(|| {
                TypexError::new(ErrorCode::InvalidRequest, format!("未知模型 {model_id}"))
            })?;
        let mut active = downloads.0.lock().unwrap();
        if active
            .get(&model_id)
            .is_some_and(|h| !h.inner().is_finished())
        {
            return Ok(()); // 已在下载中，幂等
        }
        let bytes_total: u64 = entry.files.iter().map(|f| f.bytes).sum();
        let handle = app.clone();
        let id = model_id.clone();
        let task = tauri::async_runtime::spawn(async move {
            let client = reqwest::Client::new();
            let dir = data_dir.join("models").join(&entry.id);
            // 逐文件下载，跨文件累计进度（前一文件字节 + 当前文件进度）
            let mut base: u64 = 0;
            let mut err: Option<String> = None;
            for file in &entry.files {
                let emitted = {
                    let handle = handle.clone();
                    let id = id.clone();
                    let fbytes = file.bytes;
                    Box::new(move |p: download::Progress| {
                        let _ = crate::app::events::LocalDownloadProgressEvent {
                            model_id: id.clone(),
                            bytes_done: base + p.downloaded.min(fbytes),
                            bytes_total,
                            done: false,
                            error: None,
                        }
                        .emit(&handle);
                    }) as download::ProgressFn
                };
                if let Err(e) = download::download_model_file_with_source(
                    &client,
                    &entry.sources,
                    file,
                    &dir,
                    source,
                    Some(emitted),
                )
                .await
                {
                    err = Some(e.to_string());
                    break;
                }
                base += file.bytes;
            }
            let _ = crate::app::events::LocalDownloadProgressEvent {
                model_id: id,
                bytes_done: if err.is_none() { bytes_total } else { base },
                bytes_total,
                done: true,
                error: err,
            }
            .emit(&handle);
        });
        active.insert(model_id, task);
        Ok(())
    }
    #[cfg(not(feature = "local-models"))]
    {
        let _ = (app, downloads, settings, model_id, source);
        Err(TypexError::new(ErrorCode::NotConfigured, "本地模型未启用"))
    }
}

/// 取消进行中的下载（.part 保留，下次续传）。
#[tauri::command]
#[specta::specta]
pub fn cancel_local_download(
    app: tauri::AppHandle,
    downloads: State<'_, crate::app::LocalDownloads>,
    model_id: String,
) -> Result<(), TypexError> {
    #[cfg(feature = "local-models")]
    {
        use tauri_specta::Event as _;
        if let Some(task) = downloads.0.lock().unwrap().remove(&model_id) {
            task.abort();
            // 被 abort 的任务发不出终态事件，这里代发（error = "cancelled"）
            let _ = crate::app::events::LocalDownloadProgressEvent {
                model_id,
                bytes_done: 0,
                bytes_total: 0,
                done: true,
                error: Some("cancelled".into()),
            }
            .emit(&app);
        }
        Ok(())
    }
    #[cfg(not(feature = "local-models"))]
    {
        let _ = (app, downloads, model_id);
        Err(TypexError::new(ErrorCode::NotConfigured, "本地模型未启用"))
    }
}

/// 删除已下载模型。被某个 local 档案引用且 !force → InvalidRequest（前端警告后带 force 重试）。
#[tauri::command]
#[specta::specta]
pub async fn delete_local_model(
    app: tauri::AppHandle,
    settings: SettingsState<'_>,
    model_id: String,
    force: bool,
) -> Result<(), TypexError> {
    #[cfg(feature = "local-models")]
    {
        use crate::local::{download, manifest};
        let referenced: Vec<String> = settings
            .get()
            .profiles
            .iter()
            .filter(|p| p.kind == crate::types::ProviderKind::Local && p.model == model_id)
            .map(|p| p.label.clone())
            .collect();
        if !referenced.is_empty() && !force {
            return Err(TypexError::new(
                ErrorCode::InvalidRequest,
                format!("模型被档案引用：{}", referenced.join("、")),
            ));
        }
        let entry = manifest::catalog()
            .into_iter()
            .find(|m| m.id == model_id)
            .ok_or_else(|| {
                TypexError::new(ErrorCode::InvalidRequest, format!("未知模型 {model_id}"))
            })?;
        let data_dir = local_models_data_dir(&app)?;
        download::delete_model(&data_dir, &entry)
            .await
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("删除失败: {e}")))?;
        Ok(())
    }
    #[cfg(not(feature = "local-models"))]
    {
        let _ = (app, settings, model_id, force);
        Err(TypexError::new(ErrorCode::NotConfigured, "本地模型未启用"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn profile(capability: ProviderCapability, kind: ProviderKind) -> ProviderProfile {
        ProviderProfile {
            id: "p".into(),
            capability,
            kind,
            label: "p".into(),
            base_url: "https://api.example.com/v1".into(),
            model: "m".into(),
            credentials: HashMap::new(),
            extra_headers: HashMap::new(),
            extra_form: HashMap::new(),
            timeout_ms: 30_000,
            options: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn assistant_window_ready_waits_until_marked() {
        let ready = Arc::new(AssistantWindowReady::default());
        let waiter = {
            let ready = ready.clone();
            tokio::spawn(async move { ready.wait_ready(std::time::Duration::from_secs(1)).await })
        };

        tokio::task::yield_now().await;
        ready.mark_ready();

        assert!(waiter.await.unwrap());
    }

    #[tokio::test]
    async fn assistant_window_ready_timeout_returns_false() {
        let ready = AssistantWindowReady::default();
        assert!(!ready.wait_ready(std::time::Duration::from_millis(1)).await);
    }

    #[test]
    fn profile_kind_must_match_capability() {
        assert!(profile_kind_matches_capability(&profile(
            ProviderCapability::Stt,
            ProviderKind::OpenaiCompat,
        )));
        assert!(profile_kind_matches_capability(&profile(
            ProviderCapability::Llm,
            ProviderKind::Responses,
        )));
        assert!(!profile_kind_matches_capability(&profile(
            ProviderCapability::Stt,
            ProviderKind::ChatCompletions,
        )));
    }

    #[test]
    fn slot_activation_requires_matching_capability() {
        let llm = profile(ProviderCapability::Llm, ProviderKind::ChatCompletions);
        assert!(ensure_profile_compatible(SlotKind::Translate, &llm).is_ok());
        assert!(ensure_profile_compatible(SlotKind::Stt, &llm).is_err());
    }
}
