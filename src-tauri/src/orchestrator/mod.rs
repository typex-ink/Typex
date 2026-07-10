//! Orchestrator：唯一的业务流程所有者（06 §3）。
//! 状态机（session.rs 纯函数）+ 执行器（本文件）：Effect dispatch 到各 service。
pub mod assistant;
pub mod pipeline;
pub mod session;

use crate::audio::{AudioService, Recording};
use crate::error::{ErrorCode, TypexError};
use crate::hotkey::HotkeyEvent;
use crate::inject::InjectorChain;
use crate::providers::ProviderRegistry;
use crate::providers::stt::{AudioInput, SttOptions};
use crate::settings::SettingsService;
use crate::types::{SessionMode, SessionSnapshot, SlotKind};
use session::{Effect, Event, State, advance};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// 快照推送回调（app 层注入：emit SessionSnapshotEvent；测试注入采集器）。
pub type SnapshotSink = Box<dyn Fn(SessionSnapshot) + Send + Sync>;

pub struct Orchestrator {
    pub settings: Arc<SettingsService>,
    pub audio: Arc<AudioService>,
    pub injector: Arc<InjectorChain>,
    pub registry: Arc<ProviderRegistry>,
    pub snapshot_sink: SnapshotSink,
    pub level_sink: Box<dyn Fn(Vec<f32>) + Send + Sync>,
    /// 最近一次成功注入的结果（托盘「复制上次结果」共享，02 F-7）
    pub last_result: Arc<std::sync::Mutex<Option<String>>>,
    /// 助手服务（F-3）；弹窗流式经自身 sink，呼出回调也在其内（ADR-23）
    pub assistant: Option<Arc<assistant::AssistantService>>,
    /// 助手键按下时读到的选中文本（录音开始时读取，处理阶段消费）
    pub pending_selection: Arc<std::sync::Mutex<Option<String>>>,
    /// 选区读取是否失败（读取报错 ≠ 无选区；弹窗降级提示用）
    pub selection_read_failed: Arc<std::sync::atomic::AtomicBool>,
    /// 选中文本读取器
    pub selection: Arc<dyn crate::selection::SelectionReader>,
    /// 历史记录服务（None = 未启用）
    pub history: Option<Arc<crate::history::HistoryService>>,
}

/// HUD/前端发来的会话控制命令（06 §10.1 会话组）。
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SessionCommand {
    Cancel,
    Retry,
    Dismiss,
    CopyTranscript,
    InjectOriginal,
}

/// 命令入口句柄（Tauri State 持有）。
#[derive(Clone)]
pub struct SessionCommander(pub mpsc::UnboundedSender<SessionCommand>);

/// 执行器内部状态。
struct Exec {
    state: State,
    next_session_id: u64,
    /// 会话音频（不丢话铁律：失败重试期间保留）
    audio_store: HashMap<u64, Recording>,
    /// 会话原始转写（历史记录用：id → (transcript, duration_ms)）
    transcript_store: HashMap<u64, (String, u64)>,
    recording_started: Option<Instant>,
    /// 录音开始时的前台应用名（历史 app_name / prompt 上下文 / F-11 预留）
    target_app: Option<String>,
    /// Windows 前台 HWND/PID 的进程内身份；不进入日志、历史或 IPC。
    target_focus: Option<crate::platform::focus::FocusTarget>,
    tx: mpsc::UnboundedSender<Event>,
}

impl Orchestrator {
    pub async fn run(
        self: Arc<Self>,
        mut hotkeys: mpsc::UnboundedReceiver<HotkeyEvent>,
        mut commands: mpsc::UnboundedReceiver<SessionCommand>,
    ) {
        let (tx, mut internal_rx) = mpsc::unbounded_channel::<Event>();
        let mut exec = Exec {
            state: State::Idle,
            next_session_id: 1,
            audio_store: HashMap::new(),
            transcript_store: HashMap::new(),
            recording_started: None,
            target_app: None,
            target_focus: None,
            tx: tx.clone(),
        };

        // 电平转发 task（audio worker → 前端）
        let (level_tx, mut level_rx) = mpsc::unbounded_channel::<Vec<f32>>();
        let mut audio_failures = self.audio.subscribe_failures();
        {
            let this = self.clone();
            tokio::spawn(async move {
                while let Some(levels) = level_rx.recv().await {
                    (this.level_sink)(levels);
                }
            });
        }

        loop {
            let event = tokio::select! {
                biased;
                Some(ev) = internal_rx.recv() => Some(ev),
                failure = audio_failures.recv() => failure.ok().map(|failure| Event::RecordingFailed {
                    session_id: failure.session_id,
                    error: failure.error,
                }),
                Some(hk) = hotkeys.recv() => self.map_hotkey(hk, &mut exec),
                Some(cmd) = commands.recv() => Some(match cmd {
                    SessionCommand::Cancel => Event::Esc,
                    SessionCommand::Retry => Event::Retry,
                    SessionCommand::Dismiss => Event::Dismiss,
                    SessionCommand::CopyTranscript => Event::CopyTranscriptRequested,
                    SessionCommand::InjectOriginal => Event::InjectOriginalRequested,
                }),
                else => break,
            };
            let Some(event) = event else { continue };

            // 历史记录素材：转写稿 + 录音时长（成功注入时写库）
            if let Event::SttResult {
                session_id,
                transcript,
            } = &event
            {
                let dur = exec
                    .audio_store
                    .get(session_id)
                    .map(|r| r.duration_ms)
                    .unwrap_or(0);
                exec.transcript_store
                    .insert(*session_id, (transcript.clone(), dur));
            }
            // 成功注入 → 写历史（F-7）
            if let Event::InjectDone { session_id } = &event {
                self.record_history(*session_id, &exec);
            }

            let threshold = self.settings.get().hotkeys.hold_threshold_ms;
            let (new_state, effects) = advance(exec.state.clone(), event, threshold);
            exec.state = new_state;
            for effect in effects {
                self.dispatch(effect, &mut exec, &level_tx);
            }
        }
    }

    /// hotkey 语义事件 → 状态机事件。
    fn map_hotkey(&self, hk: HotkeyEvent, exec: &mut Exec) -> Option<Event> {
        match hk {
            HotkeyEvent::TriggerDown { mode } => {
                let id = exec.next_session_id;
                // 仅 Idle/Failed 会真正开新会话；id 消耗与否由状态机决定，
                // 这里预分配即可（未消耗的 id 复用无害——单调性由 next_session_id 保证）
                if matches!(exec.state, State::Idle | State::Failed { .. }) {
                    exec.next_session_id += 1;
                }
                Some(Event::TriggerDown {
                    mode,
                    next_session_id: id,
                })
            }
            HotkeyEvent::ModeUpgraded { mode } => Some(Event::ModeUpgraded { mode }),
            HotkeyEvent::TriggerUp { held_ms } => Some(Event::TriggerUp { held_ms }),
            HotkeyEvent::Yielded => Some(Event::Yielded),
            HotkeyEvent::EscPressed => {
                if self.settings.get().dictation.esc_cancels {
                    Some(Event::Esc)
                } else {
                    None
                }
            }
        }
    }

    fn dispatch(
        &self,
        effect: Effect,
        exec: &mut Exec,
        level_tx: &mpsc::UnboundedSender<Vec<f32>>,
    ) {
        match effect {
            Effect::StartRecording => {
                let mic = self.settings.get().dictation.microphone.clone();
                let Some(session_id) = exec.state.session_id() else {
                    return;
                };
                exec.recording_started = Some(Instant::now());
                // 采样注入目标应用（02 F-7：录音开始时的前台应用即注入目标）
                exec.target_focus = crate::platform::focus::FocusTarget::capture();
                exec.target_app = exec
                    .target_focus
                    .as_ref()
                    .and_then(crate::platform::focus::FocusTarget::app_name);
                // 助手模式的选区读取推迟到触发键松开（CallStt 时并发执行，06 §7.6-5）：
                // 剪贴板降级的模拟 Cmd+C 在按住期间会触发组合键让路、误取消本会话
                if exec.state.mode() == Some(SessionMode::Assistant) {
                    *self.pending_selection.lock().unwrap() = None;
                }
                match self.audio.start(session_id, &mic, level_tx.clone()) {
                    Ok(Some(migrated_device_id)) => {
                        if let Err(error) = self.settings.mutate(|settings| {
                            settings.dictation.microphone = migrated_device_id;
                        }) {
                            tracing::warn!(
                                error_code = ?error.code,
                                "failed to persist migrated microphone endpoint ID"
                            );
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::error!(
                            error_code = ?error.code,
                            "recording failed to start"
                        );
                        let _ = exec.tx.send(Event::RecordingFailed { session_id, error });
                    }
                }
            }
            Effect::CancelRecording => {
                self.audio.cancel();
                exec.recording_started = None;
                self.emit_snapshot(exec); // HUD 隐藏
            }
            Effect::StopRecording { .. } => { /* CallStt 已含停止语义 */ }
            Effect::CallStt { session_id } => {
                // 停止录音（若在录）并取音频；重试路径直接用 audio_store
                let was_recording = self.audio.is_recording();
                let recording = if was_recording {
                    exec.recording_started = None;
                    match self.audio.stop() {
                        Ok(rec) => {
                            exec.audio_store.insert(session_id, rec.clone());
                            Some(rec)
                        }
                        Err(e) => {
                            let _ = exec.tx.send(Event::RecordingFailed {
                                session_id,
                                error: e,
                            });
                            None
                        }
                    }
                } else {
                    exec.audio_store.get(&session_id).cloned()
                };
                let Some(rec) = recording else { return };
                if rec.duration_ms < 200 {
                    let _ = exec.tx.send(Event::SttFailed {
                        session_id,
                        error: TypexError::new(ErrorCode::NoSpeech, "录音过短"),
                    });
                    return;
                }
                // 助手模式：触发键已松开，此刻读选区（06 §7.6-5——按住期间读会被
                // 剪贴板降级的模拟 Cmd+C 触发组合键让路）；与 STT 并发，不增加延迟。
                // 重试路径（!was_recording）沿用首次读到的选区。
                let assistant_read = (was_recording
                    && exec.state.mode() == Some(SessionMode::Assistant))
                .then(|| {
                    (
                        self.selection.clone(),
                        self.pending_selection.clone(),
                        self.selection_read_failed.clone(),
                        exec.target_focus.clone(),
                    )
                });
                let registry = self.registry.clone();
                let settings = self.settings.get();
                let lang = settings.dictation.language.clone();
                let stt_prompt = settings.dictionary.stt_prompt();
                let tx = exec.tx.clone();
                tokio::spawn(async move {
                    let selection_fut = async {
                        if let Some((reader, pending, failed, target)) = assistant_read {
                            let outcome = tokio::task::spawn_blocking(move || {
                                reader.read_targeted(target.as_ref())
                            })
                            .await;
                            let (result, read_failed) = match outcome {
                                Ok(Ok(sel)) => (sel, false),
                                _ => (None, true), // 读取报错 → 降级为普通提问（05 §4）
                            };
                            failed.store(read_failed, std::sync::atomic::Ordering::Relaxed);
                            *pending.lock().unwrap() = result;
                        }
                    };
                    let stt_fut = async {
                        let stt = match registry.stt_for(SlotKind::Stt) {
                            Ok(s) => s,
                            Err(e) => {
                                return Event::SttFailed {
                                    session_id,
                                    error: e,
                                };
                            }
                        };
                        let result = crate::providers::stt::transcribe_auto_chunk(
                            stt.as_ref(),
                            AudioInput {
                                wav_16k_mono: rec.wav_16k_mono,
                                duration_ms: rec.duration_ms,
                            },
                            SttOptions {
                                language: Some(lang),
                                prompt: stt_prompt,
                                temperature: None,
                            },
                        )
                        .await;
                        match result {
                            Ok(t) if t.text.trim().is_empty() => Event::SttFailed {
                                session_id,
                                error: TypexError::new(ErrorCode::NoSpeech, "没有听到声音"),
                            },
                            Ok(t) => Event::SttResult {
                                session_id,
                                transcript: t.text.trim().to_string(),
                            },
                            Err(e) => Event::SttFailed {
                                session_id,
                                error: e.into(),
                            },
                        }
                    };
                    // join：确保选区已写入 pending 再发 SttResult（CallProcess 会立即消费）
                    let ((), event) = tokio::join!(selection_fut, stt_fut);
                    let _ = tx.send(event);
                });
            }
            Effect::CallProcess {
                session_id,
                mode,
                transcript,
            } => {
                // 助手模式：转写结果 = 语音指令 → 助手服务分流（ADR-23）：
                // 改写型 → ProcessResult（注入替换选区）；回答型 → 交回答弹窗，会话结束
                if mode == SessionMode::Assistant {
                    let Some(assistant) = self.assistant.clone() else {
                        let _ = exec.tx.send(Event::ProcessFailed {
                            session_id,
                            error: TypexError::new(ErrorCode::NotConfigured, "助手服务未装配"),
                        });
                        return;
                    };
                    // clone 而非 take：失败重试时仍可携带同一选区上下文（下次录音开始时重置）
                    let selection = self.pending_selection.lock().unwrap().clone();
                    let read_failed = self
                        .selection_read_failed
                        .load(std::sync::atomic::Ordering::Relaxed);
                    let prompt_context = pipeline::PromptContext::new(exec.target_app.clone());
                    let tx = exec.tx.clone();
                    tokio::spawn(async move {
                        let event = match assistant
                            .run(transcript, selection, read_failed, prompt_context)
                            .await
                        {
                            Ok(assistant::AssistantOutcome::Rewrite(text)) => {
                                Event::ProcessResult { session_id, text }
                            }
                            Ok(assistant::AssistantOutcome::HandedOff) => {
                                Event::AssistantHandedOff { session_id }
                            }
                            Err(error) => Event::ProcessFailed { session_id, error },
                        };
                        let _ = tx.send(event);
                    });
                    return;
                }
                let tx = exec.tx.clone();
                let settings = self.settings.get();
                let registry = self.registry.clone();
                let prompt_context = pipeline::PromptContext::new(exec.target_app.clone());
                tokio::spawn(async move {
                    let event = match pipeline::process(
                        mode,
                        transcript,
                        &settings,
                        &registry,
                        &prompt_context,
                    )
                    .await
                    {
                        pipeline::ProcessOutcome::Done(text) => {
                            Event::ProcessResult { session_id, text }
                        }
                        pipeline::ProcessOutcome::Degraded(original) => Event::ProcessDegraded {
                            session_id,
                            original,
                        },
                        pipeline::ProcessOutcome::Failed(error) => {
                            Event::ProcessFailed { session_id, error }
                        }
                    };
                    let _ = tx.send(event);
                });
            }
            Effect::Inject { session_id, text } => {
                let injector = self.injector.clone();
                let tx = exec.tx.clone();
                let method = self.settings.get().dictation.inject_method;
                let target = exec.target_focus.clone();
                *self.last_result.lock().unwrap() = Some(text.clone());
                // enigo/剪贴板是阻塞调用 → blocking 线程
                tokio::task::spawn_blocking(move || {
                    let result =
                        if crate::platform::focus::captured_target_is_current(target.as_ref()) {
                            injector.inject_with_target(&text, method, target.as_ref())
                        } else {
                            Err(TypexError::new(
                                ErrorCode::NoFocus,
                                "foreground target changed before injection",
                            ))
                        };
                    let event = match result {
                        Ok(()) => Event::InjectDone { session_id },
                        Err(e) => Event::InjectFailed {
                            session_id,
                            error: e,
                        },
                    };
                    let _ = tx.send(event);
                });
            }
            Effect::EmitUi => self.emit_snapshot(exec),
            Effect::EmitBusyHint => {
                // 重按忽略：HUD 轻晃 + 「正在处理上一条…」（05 §3.3）
                self.emit_snapshot_with(exec, true);
            }
            Effect::PlayChime(chime) => {
                let g = self.settings.get().general;
                if g.chimes_enabled {
                    let kind = match chime {
                        session::Chime::Start => crate::audio::chime::ChimeKind::Start,
                        session::Chime::Success => crate::audio::chime::ChimeKind::Success,
                        session::Chime::Error => crate::audio::chime::ChimeKind::Error,
                    };
                    crate::audio::chime::play(kind, g.chimes_volume);
                }
            }
            Effect::CopyToClipboard(text) => {
                // Injection fallback must be confirmed before the following EmitUi claims the
                // result is available on the clipboard. The Windows implementation uses bounded
                // retries and a valid owner HWND.
                if let Err(copy_error) = crate::inject::copy_text_to_clipboard(&text) {
                    tracing::warn!(
                        error_code = ?copy_error.code,
                        "failed to copy injection fallback to clipboard"
                    );
                    if let State::Failed { error, .. } = &mut exec.state {
                        *error = copy_error;
                    }
                }
            }
            Effect::ReleaseAudio { session_id } => {
                exec.audio_store.remove(&session_id);
                exec.transcript_store.remove(&session_id);
            }
        }
    }

    fn record_history(&self, session_id: u64, exec: &Exec) {
        let Some(history) = &self.history else { return };
        if !self.settings.get().history.enabled {
            return;
        }
        let Some((transcript, duration_ms)) = exec.transcript_store.get(&session_id) else {
            return;
        };
        let result = self.last_result.lock().unwrap().clone().unwrap_or_default();
        let mode = exec.state.mode().unwrap_or(SessionMode::Dictation);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let app_name = exec.target_app.clone().unwrap_or_default();
        if let Err(e) = history.insert(
            now,
            mode,
            transcript,
            &result,
            &app_name,
            *duration_ms as u32,
        ) {
            tracing::warn!("写历史失败: {}", e.message);
        }
    }

    fn emit_snapshot(&self, exec: &Exec) {
        self.emit_snapshot_with(exec, false);
    }

    fn emit_snapshot_with(&self, exec: &Exec, busy_hint: bool) {
        let mut snap = snapshot_of(&exec.state, exec.recording_started);
        snap.busy_hint = busy_hint;
        // 快照补全设置态字段：原样模式标注、翻译方向徽标（05 §3.2）
        let s = self.settings.get();
        snap.verbatim = !s.dictation.polish_enabled;
        if snap.mode == SessionMode::Translation {
            snap.translation_direction = Some(direction_label(
                &s.translation.source_language,
                &s.translation.target_language,
            ));
        }
        (self.snapshot_sink)(snap);
    }
}

/// 翻译方向徽标：如「中 → EN」（05 §3.2）。
fn direction_label(source: &str, target: &str) -> String {
    fn short(lang: &str) -> String {
        match lang {
            l if l.starts_with("中文") => "中".into(),
            "English" => "EN".into(),
            "日本語" => "日".into(),
            "한국어" => "한".into(),
            "Français" => "FR".into(),
            "Deutsch" => "DE".into(),
            "Español" => "ES".into(),
            "Русский" => "RU".into(),
            other => other.chars().take(2).collect(),
        }
    }
    format!("{} → {}", short(source), short(target))
}

/// State → SessionSnapshot 投影。
fn snapshot_of(state: &State, recording_started: Option<Instant>) -> SessionSnapshot {
    let mut snap = SessionSnapshot::idle();
    snap.session_id = state.session_id().unwrap_or(0);
    if let Some(m) = state.mode() {
        snap.mode = m;
    }
    snap.phase = state.phase();
    match state {
        State::Recording { .. } => {
            snap.recording_ms = recording_started
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);
        }
        State::Processing { .. } => {
            snap.processing_step = Some("processing".into());
            snap.has_transcript = true;
        }
        State::Injecting { unpolished, .. } => {
            snap.unpolished = *unpolished;
            snap.has_transcript = true;
        }
        State::Failed {
            error,
            stage,
            transcript,
            ..
        } => {
            snap.error = Some(error.code);
            snap.failed_stage = Some(*stage);
            snap.has_transcript = transcript.is_some();
        }
        _ => {}
    }
    snap
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionPhase;

    #[test]
    fn snapshot_projection_failed_keeps_transcript_flag() {
        let s = State::Failed {
            session_id: 3,
            mode: SessionMode::Translation,
            stage: crate::types::FailedStage::Processing,
            error: TypexError::new(ErrorCode::Timeout, "t"),
            transcript: Some("x".into()),
        };
        let snap = snapshot_of(&s, None);
        assert_eq!(snap.phase, SessionPhase::Failed);
        assert!(snap.has_transcript);
        assert_eq!(snap.error, Some(ErrorCode::Timeout));
    }
}
