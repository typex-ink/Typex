//! Orchestrator：唯一的业务流程所有者（06 §3）。
//! 状态机（session.rs 纯函数）+ 执行器（本文件）：Effect dispatch 到各 service。
pub mod assistant;
pub mod pipeline;
pub mod session;

use crate::audio::{AudioService, CandidatePromotion, Recording};
use crate::error::{ErrorCode, TypexError};
use crate::hotkey::HotkeyEvent;
use crate::inject::{InjectionLatch, InjectionOutcome, InjectorChain};
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
    /// 助手触发 chord 释放后读到的选中文本（与 STT 并发读取，处理阶段消费）
    pub pending_selection: Arc<std::sync::Mutex<Option<String>>>,
    /// 选区读取是否失败（读取报错 ≠ 无选区；弹窗降级提示用）
    pub selection_read_failed: Arc<std::sync::atomic::AtomicBool>,
    /// 选中文本读取器
    pub selection: Arc<dyn crate::selection::SelectionReader>,
    /// 历史记录服务（None = 未启用）
    pub history: Option<Arc<crate::history::HistoryService>>,
}

/// HUD/前端发来的会话控制命令（06 §10.1 会话组）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
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
pub struct SessionCommander(mpsc::UnboundedSender<SessionControl>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionControl {
    User(SessionCommand),
    CancelRecording,
}

impl SessionCommander {
    pub(crate) fn channel() -> (Self, mpsc::UnboundedReceiver<SessionControl>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self(tx), rx)
    }

    pub fn send(&self, command: SessionCommand) {
        let _ = self.0.send(SessionControl::User(command));
    }

    pub fn cancel_recording(&self) {
        let _ = self.0.send(SessionControl::CancelRecording);
    }
}

/// 执行器内部状态。
struct Exec {
    state: State,
    next_session_id: u64,
    /// 会话音频（不丢话铁律：失败重试期间保留）
    audio_store: HashMap<u64, Recording>,
    /// Blocking audio finalization writes here immediately before RecordingFinished.
    finished_audio: Arc<std::sync::Mutex<HashMap<u64, crate::error::Result<Recording>>>>,
    /// 会话原始转写（历史记录用：id → (transcript, duration_ms)）
    transcript_store: HashMap<u64, (String, u64)>,
    recording_started: Option<Instant>,
    /// 录音开始时的前台应用名（历史 app_name / prompt 上下文 / F-11 预留）
    target_app: Option<String>,
    /// Windows 前台 HWND/PID 的进程内身份；不进入日志、历史或 IPC。
    target_focus: Option<crate::platform::focus::FocusTarget>,
    /// Windows raw-key candidate waiting for its delayed semantic confirmation.
    pending_candidate_token: Option<u64>,
    /// Candidate selected by the current TriggerDown's StartRecording effect.
    promote_candidate_token: Option<u64>,
    /// Abort handles for all asynchronous work owned by a session.
    task_handles: HashMap<u64, Vec<tokio::task::AbortHandle>>,
    /// Injection cancellation remains shared with spawn_blocking after its JoinHandle is aborted.
    injection_latches: HashMap<u64, Arc<InjectionLatch>>,
    tx: mpsc::UnboundedSender<Event>,
}

impl Exec {
    fn track_task(&mut self, session_id: u64, handle: &tokio::task::JoinHandle<()>) {
        self.task_handles
            .entry(session_id)
            .or_default()
            .push(handle.abort_handle());
    }

    fn abort_tasks(&mut self, session_id: u64) {
        if let Some(handles) = self.task_handles.remove(&session_id) {
            for handle in handles {
                handle.abort();
            }
        }
    }

    fn release_session(&mut self, session_id: u64) {
        self.abort_tasks(session_id);
        self.injection_latches.remove(&session_id);
        self.finished_audio.lock().unwrap().remove(&session_id);
        self.audio_store.remove(&session_id);
        self.transcript_store.remove(&session_id);
    }
}

impl Orchestrator {
    pub(crate) async fn run(
        self: Arc<Self>,
        mut hotkeys: mpsc::UnboundedReceiver<HotkeyEvent>,
        mut commands: mpsc::UnboundedReceiver<SessionControl>,
    ) {
        let (tx, mut internal_rx) = mpsc::unbounded_channel::<Event>();
        let mut exec = Exec {
            state: State::Idle,
            next_session_id: 1,
            audio_store: HashMap::new(),
            finished_audio: Arc::new(std::sync::Mutex::new(HashMap::new())),
            transcript_store: HashMap::new(),
            recording_started: None,
            target_app: None,
            target_focus: None,
            pending_candidate_token: None,
            promote_candidate_token: None,
            task_handles: HashMap::new(),
            injection_latches: HashMap::new(),
            tx: tx.clone(),
        };

        // 电平转发 task（audio worker → 前端）
        let (level_tx, mut level_rx) = mpsc::unbounded_channel::<Vec<f32>>();
        let mut audio_failures = self.audio.subscribe_failures();
        let mut audio_ready = self.audio.subscribe_ready();
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
                Some(control) = commands.recv() => {
                    if matches!(
                        control,
                        SessionControl::User(SessionCommand::Cancel)
                            | SessionControl::CancelRecording
                    ) {
                        self.audio.cancel_pending_candidate();
                        exec.pending_candidate_token = None;
                        exec.promote_candidate_token = None;
                    }
                    match control {
                        SessionControl::User(SessionCommand::Cancel) => {
                            self.begin_cancel(&mut exec)
                        }
                        SessionControl::User(SessionCommand::Retry) => Some(Event::Retry),
                        SessionControl::User(SessionCommand::Dismiss) => Some(Event::Dismiss),
                        SessionControl::User(SessionCommand::CopyTranscript) => {
                            Some(Event::CopyTranscriptRequested)
                        }
                        SessionControl::User(SessionCommand::InjectOriginal) => {
                            Some(Event::InjectOriginalRequested)
                        }
                        SessionControl::CancelRecording => matches!(
                            exec.state,
                            State::Recording { .. }
                        )
                        .then_some(Event::Esc),
                    }
                },
                Some(hk) = hotkeys.recv() => self.map_hotkey(hk, &mut exec),
                Some(ev) = internal_rx.recv() => Some(ev),
                failure = audio_failures.recv() => failure.ok().map(|failure| Event::RecordingFailed {
                    session_id: failure.session_id,
                    error: failure.error,
                }),
                ready = audio_ready.recv() => {
                    if let Ok(ready) = ready
                        && let Some(device_id) = ready.migrated_device_id
                    {
                        self.persist_migrated_microphone(device_id);
                    }
                    None
                },
                else => break,
            };
            let Some(event) = event else { continue };

            let event = match event {
                Event::RecordingFinished { session_id } => {
                    let result = exec.finished_audio.lock().unwrap().remove(&session_id);
                    if matches!(
                        exec.state,
                        State::Transcribing {
                            session_id: active,
                            ..
                        } if active == session_id
                    ) {
                        match result {
                            Some(Ok(recording)) => {
                                exec.audio_store.insert(session_id, recording);
                                Event::RecordingFinished { session_id }
                            }
                            Some(Err(error)) => Event::RecordingFailed { session_id, error },
                            None => continue,
                        }
                    } else {
                        continue;
                    }
                }
                event => event,
            };

            // 历史记录素材：转写稿 + 录音时长（成功注入时写库）
            if let Event::SttResult {
                session_id,
                transcript,
            } = &event
                && matches!(
                    exec.state,
                    State::Transcribing {
                        session_id: active,
                        ..
                    } if active == *session_id
                )
            {
                let dur = exec
                    .audio_store
                    .get(session_id)
                    .map(|r| r.duration_ms)
                    .unwrap_or(0);
                exec.transcript_store
                    .insert(*session_id, (transcript.clone(), dur));
            }
            // 成功注入 → 更新上次结果并写历史（F-7）。
            if let Event::InjectDone { session_id } = &event {
                self.record_injected_history(*session_id, &exec);
            }
            // 完整成功回答写助手历史；弹窗内错误用 None，明确不写。
            if let Event::AssistantHandedOff {
                session_id,
                answer: Some(answer),
            } = &event
                && matches!(
                    exec.state,
                    State::Processing {
                        session_id: active,
                        mode: SessionMode::Assistant,
                        ..
                    } if active == *session_id
                )
            {
                self.record_history_result(*session_id, SessionMode::Assistant, answer, &exec);
            }

            let threshold = self.settings.get().hotkeys.hold_threshold_ms;
            let (new_state, effects) = advance(exec.state.clone(), event, threshold);
            exec.state = new_state;
            for effect in effects {
                self.dispatch(effect, &mut exec, &level_tx);
            }
        }
        self.audio.cancel();
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
            HotkeyEvent::CaptureCandidateStarted { token } => {
                if matches!(exec.state, State::Idle | State::Failed { .. }) {
                    let settings = self.settings.get();
                    if self.audio.prepare_candidate(
                        token,
                        &settings.dictation.microphone,
                        settings.dictation.vad,
                    ) {
                        exec.pending_candidate_token = Some(token);
                    }
                }
                None
            }
            HotkeyEvent::CaptureCandidatePromoted { token, mode } => {
                if exec.pending_candidate_token == Some(token)
                    && matches!(exec.state, State::Idle | State::Failed { .. })
                {
                    exec.pending_candidate_token = None;
                    exec.promote_candidate_token = Some(token);
                } else {
                    self.audio.cancel_candidate(token);
                }
                let id = exec.next_session_id;
                if matches!(exec.state, State::Idle | State::Failed { .. }) {
                    exec.next_session_id += 1;
                }
                Some(Event::TriggerDown {
                    mode,
                    next_session_id: id,
                })
            }
            HotkeyEvent::CaptureCandidateCancelled { token } => {
                self.audio.cancel_candidate(token);
                if exec.pending_candidate_token == Some(token) {
                    exec.pending_candidate_token = None;
                }
                if exec.promote_candidate_token == Some(token) {
                    exec.promote_candidate_token = None;
                }
                None
            }
            HotkeyEvent::ModeUpgraded { mode } => Some(Event::ModeUpgraded { mode }),
            HotkeyEvent::TriggerUp { held_ms } => Some(Event::TriggerUp { held_ms }),
            HotkeyEvent::Yielded => Some(Event::Yielded),
            HotkeyEvent::EscPressed => {
                if self.settings.get().dictation.esc_cancels {
                    self.begin_cancel(exec)
                } else {
                    None
                }
            }
        }
    }

    /// Atomically wins cancellation before the injection commit boundary. A committed injection
    /// must run to completion so history and UI reflect the OS input that may already have begun.
    fn begin_cancel(&self, exec: &mut Exec) -> Option<Event> {
        let session_id = exec.state.session_id()?;
        if matches!(exec.state, State::Injecting { .. })
            && exec
                .injection_latches
                .get(&session_id)
                .is_some_and(|latch| !latch.cancel())
        {
            return None;
        }
        Some(Event::Esc)
    }

    fn dispatch(
        &self,
        effect: Effect,
        exec: &mut Exec,
        level_tx: &mpsc::UnboundedSender<Vec<f32>>,
    ) {
        match effect {
            Effect::StartRecording => {
                let settings = self.settings.get();
                let mic = settings.dictation.microphone.clone();
                let vad = settings.dictation.vad;
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
                let start_result = if let Some(token) = exec.promote_candidate_token.take() {
                    match self
                        .audio
                        .promote_candidate(token, session_id, level_tx.clone())
                    {
                        Ok(CandidatePromotion::Opening) => Ok(None),
                        Ok(CandidatePromotion::Ready(migrated)) => Ok(migrated),
                        Ok(CandidatePromotion::NotFound) => {
                            self.audio.start(session_id, &mic, level_tx.clone(), vad)
                        }
                        Err(error) => Err(error),
                    }
                } else {
                    self.audio.start(session_id, &mic, level_tx.clone(), vad)
                };
                match start_result {
                    Ok(Some(migrated_device_id)) => {
                        self.persist_migrated_microphone(migrated_device_id);
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
            Effect::StopRecording { session_id } => {
                exec.recording_started = None;
                let audio = self.audio.clone();
                let finished_audio = exec.finished_audio.clone();
                let tx = exec.tx.clone();
                let assistant_read =
                    (exec.state.mode() == Some(SessionMode::Assistant)).then(|| {
                        (
                            self.selection.clone(),
                            self.pending_selection.clone(),
                            self.selection_read_failed.clone(),
                            exec.target_focus.clone(),
                        )
                    });
                let handle = tokio::spawn(async move {
                    let audio_fut = tokio::task::spawn_blocking(move || audio.stop());
                    let selection_fut = async move {
                        if let Some((reader, pending, failed, target)) = assistant_read {
                            let outcome = tokio::task::spawn_blocking(move || {
                                reader.read_targeted(target.as_ref())
                            })
                            .await;
                            let (result, read_failed) = match outcome {
                                Ok(Ok(selection)) => (selection, false),
                                _ => (None, true),
                            };
                            failed.store(read_failed, std::sync::atomic::Ordering::Relaxed);
                            *pending.lock().unwrap() = result;
                        }
                    };
                    let (audio_result, ()) = tokio::join!(audio_fut, selection_fut);
                    let audio_result = audio_result.unwrap_or_else(|error| {
                        Err(TypexError::new(
                            ErrorCode::Internal,
                            format!("音频收尾任务失败: {error}"),
                        ))
                    });
                    finished_audio
                        .lock()
                        .unwrap()
                        .insert(session_id, audio_result);
                    let _ = tx.send(Event::RecordingFinished { session_id });
                });
                exec.track_task(session_id, &handle);
            }
            Effect::CallStt { session_id } => {
                // 正常路径由 RecordingFinished 进入；失败重试直接复用 audio_store。
                let Some(rec) = exec.audio_store.get(&session_id).cloned() else {
                    return;
                };
                if rec.duration_ms < 90 {
                    let _ = exec.tx.send(Event::SttFailed {
                        session_id,
                        error: TypexError::new(ErrorCode::NoSpeech, "录音过短"),
                    });
                    return;
                }
                let registry = self.registry.clone();
                let settings = self.settings.get();
                let lang = settings.dictation.language.clone();
                let stt_prompt = settings.dictionary.stt_prompt();
                let tx = exec.tx.clone();
                let handle = tokio::spawn(async move {
                    let stt = match registry.stt_for(SlotKind::Stt) {
                        Ok(stt) => stt,
                        Err(error) => {
                            let _ = tx.send(Event::SttFailed { session_id, error });
                            return;
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
                        rec.vad,
                    )
                    .await;
                    let event = match result {
                        Ok(transcript) if transcript.text.trim().is_empty() => Event::SttFailed {
                            session_id,
                            error: TypexError::new(ErrorCode::NoSpeech, "没有听到声音"),
                        },
                        Ok(transcript) => Event::SttResult {
                            session_id,
                            transcript: transcript.text.trim().to_string(),
                        },
                        Err(error) => Event::SttFailed {
                            session_id,
                            error: error.into(),
                        },
                    };
                    let _ = tx.send(event);
                });
                exec.track_task(session_id, &handle);
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
                    let handle = tokio::spawn(async move {
                        let event = match assistant
                            .run(transcript, selection, read_failed, prompt_context)
                            .await
                        {
                            Ok(assistant::AssistantOutcome::Rewrite(text)) => {
                                Event::ProcessResult { session_id, text }
                            }
                            Ok(assistant::AssistantOutcome::HandedOff(answer)) => {
                                Event::AssistantHandedOff { session_id, answer }
                            }
                            Err(error) => Event::ProcessFailed { session_id, error },
                        };
                        let _ = tx.send(event);
                    });
                    exec.track_task(session_id, &handle);
                    return;
                }
                let tx = exec.tx.clone();
                let settings = self.settings.get();
                let registry = self.registry.clone();
                let prompt_context = pipeline::PromptContext::new(exec.target_app.clone());
                let handle = tokio::spawn(async move {
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
                exec.track_task(session_id, &handle);
            }
            Effect::Inject { session_id, text } => {
                let injector = self.injector.clone();
                let tx = exec.tx.clone();
                let method = self.settings.get().dictation.inject_method;
                let target = exec.target_focus.clone();
                let latch = Arc::new(InjectionLatch::new());
                exec.injection_latches.insert(session_id, latch.clone());
                // enigo/剪贴板是阻塞调用 → blocking 线程
                let handle = tokio::task::spawn_blocking(move || {
                    let result = injector.inject_with_target_cancellable(
                        &text,
                        method,
                        target.as_ref(),
                        &latch,
                    );
                    let event = match result {
                        Ok(InjectionOutcome::Injected) => Event::InjectDone { session_id },
                        Ok(InjectionOutcome::Cancelled) => return,
                        Err(error) => Event::InjectFailed { session_id, error },
                    };
                    let _ = tx.send(event);
                });
                exec.track_task(session_id, &handle);
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
            Effect::CancelTasks { session_id } => {
                exec.abort_tasks(session_id);
                if let Some(assistant) = &self.assistant {
                    assistant.hide_panel();
                }
            }
            Effect::ReleaseAudio { session_id } => {
                exec.release_session(session_id);
            }
        }
    }

    fn record_injected_history(&self, session_id: u64, exec: &Exec) {
        let State::Injecting {
            session_id: active,
            mode,
            text,
            ..
        } = &exec.state
        else {
            return;
        };
        if *active != session_id {
            return;
        }
        *self.last_result.lock().unwrap() = Some(text.clone());
        self.record_history_result(session_id, *mode, text, exec);
    }

    fn record_history_result(&self, session_id: u64, mode: SessionMode, result: &str, exec: &Exec) {
        let Some(history) = &self.history else { return };
        if !self.settings.get().history.enabled {
            return;
        }
        let Some((transcript, duration_ms)) = exec.transcript_store.get(&session_id) else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let app_name = exec.target_app.clone().unwrap_or_default();
        if let Err(e) = history.insert(
            now,
            mode,
            transcript,
            result,
            &app_name,
            *duration_ms as u32,
        ) {
            tracing::warn!("写历史失败: {}", e.message);
        }
    }

    fn persist_migrated_microphone(&self, device_id: String) {
        if let Err(error) = self.settings.mutate(|settings| {
            settings.dictation.microphone = device_id;
        }) {
            tracing::warn!(
                error_code = ?error.code,
                "failed to persist migrated microphone endpoint ID"
            );
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
    use crate::settings::schema::VadSettings;
    use crate::types::SessionPhase;

    struct NoSelection;

    impl crate::selection::SelectionReader for NoSelection {
        fn read(&self) -> crate::error::Result<Option<String>> {
            Ok(None)
        }
    }

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

    #[tokio::test]
    async fn delayed_audio_finish_publishes_transcribing_and_remains_cancellable() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Arc::new(SettingsService::load(dir.path().to_path_buf()));
        settings
            .mutate(|settings| {
                settings.general.chimes_enabled = false;
                settings.dictation.polish_enabled = false;
            })
            .unwrap();
        let recording = Recording {
            wav_16k_mono: Vec::new(),
            duration_ms: 100,
            vad: VadSettings::default(),
        };
        let audio = Arc::new(AudioService::with_delayed_recording(
            recording,
            std::time::Duration::from_millis(250),
        ));
        let registry = Arc::new(ProviderRegistry::new(settings.get()));
        let (snapshot_tx, mut snapshot_rx) = mpsc::unbounded_channel();
        let orchestrator = Arc::new(Orchestrator {
            settings: settings.clone(),
            audio,
            injector: Arc::new(InjectorChain::new(Vec::new())),
            registry,
            snapshot_sink: Box::new(move |snapshot| {
                let _ = snapshot_tx.send(snapshot.phase);
            }),
            level_sink: Box::new(|_| {}),
            last_result: Arc::new(std::sync::Mutex::new(None)),
            assistant: None,
            pending_selection: Arc::new(std::sync::Mutex::new(None)),
            selection_read_failed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            selection: Arc::new(NoSelection),
            history: None,
        });
        let (hotkey_tx, hotkey_rx) = mpsc::unbounded_channel();
        let (commander, command_rx) = SessionCommander::channel();
        let task = tokio::spawn(orchestrator.run(hotkey_rx, command_rx));

        hotkey_tx
            .send(HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation,
            })
            .unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_millis(100), snapshot_rx.recv())
                .await
                .unwrap(),
            Some(SessionPhase::Recording)
        );

        hotkey_tx
            .send(HotkeyEvent::TriggerUp { held_ms: 351 })
            .unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_millis(100), snapshot_rx.recv())
                .await
                .unwrap(),
            Some(SessionPhase::Transcribing)
        );

        settings
            .mutate(|settings| settings.dictation.esc_cancels = false)
            .unwrap();
        hotkey_tx.send(HotkeyEvent::EscPressed).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), snapshot_rx.recv())
                .await
                .is_err(),
            "disabled Escape must not cancel audio finalization"
        );

        commander.send(SessionCommand::Cancel);
        let idle = tokio::time::timeout(std::time::Duration::from_millis(100), async {
            loop {
                if snapshot_rx.recv().await == Some(SessionPhase::Idle) {
                    break SessionPhase::Idle;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(idle, SessionPhase::Idle);

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        while let Ok(phase) = snapshot_rx.try_recv() {
            assert_eq!(phase, SessionPhase::Idle);
        }
        task.abort();
    }
}
