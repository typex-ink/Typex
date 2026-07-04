//! Orchestrator：唯一的业务流程所有者（07 §3）。
//! 状态机（session.rs 纯函数）+ 执行器（本文件）：Effect dispatch 到各 service。
pub mod assistant;
pub mod pipeline;
pub mod session;

use crate::audio::{AudioService, Recording};
use crate::error::{ErrorCode, TypexError};
use crate::hotkey::HotkeyEvent;
use crate::inject::InjectorChain;
use crate::providers::stt::{AudioInput, SttOptions};
use crate::providers::ProviderRegistry;
use crate::settings::SettingsService;
use crate::types::{SessionMode, SessionSnapshot, SlotKind};
use session::{advance, Effect, Event, State};
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
    /// 助手服务（F-3）；快照/流式经自身 sink
    pub assistant: Option<Arc<assistant::AssistantService>>,
    /// 助手键按下时读到的选中文本（录音开始时读取，转写完成后消费）
    pub pending_selection: Arc<std::sync::Mutex<Option<String>>>,
    /// 显示助手面板回调（app 层注入）
    pub show_assistant_panel: Box<dyn Fn() + Send + Sync>,
    /// 选中文本读取器
    pub selection: Arc<dyn crate::selection::SelectionReader>,
}

/// HUD/前端发来的会话控制命令（07 §10.1 会话组）。
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
    recording_started: Option<Instant>,
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
            recording_started: None,
            tx: tx.clone(),
        };

        // 电平转发 task（audio worker → 前端）
        let (level_tx, mut level_rx) = mpsc::unbounded_channel::<Vec<f32>>();
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
                Some(hk) = hotkeys.recv() => self.map_hotkey(hk, &mut exec),
                Some(cmd) = commands.recv() => Some(match cmd {
                    SessionCommand::Cancel => Event::Esc,
                    SessionCommand::Retry => Event::Retry,
                    SessionCommand::Dismiss => Event::Dismiss,
                    SessionCommand::CopyTranscript => Event::CopyTranscriptRequested,
                    SessionCommand::InjectOriginal => Event::InjectOriginalRequested,
                }),
                Some(ev) = internal_rx.recv() => Some(ev),
                else => break,
            };
            let Some(event) = event else { continue };

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
                Some(Event::TriggerDown { mode, next_session_id: id })
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
                exec.recording_started = Some(Instant::now());
                // 助手模式：录音开始时后台读选中文本（F-3a 上下文；不阻塞录音）
                if exec.state.mode() == Some(SessionMode::Assistant) {
                    let selection = self.selection.clone();
                    let pending = self.pending_selection.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = selection.read().ok().flatten();
                        *pending.lock().unwrap() = result;
                    });
                }
                if let Err(e) = self.audio.start(&mic, level_tx.clone()) {
                    tracing::error!("录音启动失败: {}", e.message);
                    if let Some(sid) = exec.state.session_id() {
                        let _ = exec.tx.send(Event::SttFailed { session_id: sid, error: e });
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
                let recording = if self.audio.is_recording() {
                    exec.recording_started = None;
                    match self.audio.stop() {
                        Ok(rec) => {
                            exec.audio_store.insert(session_id, rec.clone());
                            Some(rec)
                        }
                        Err(e) => {
                            let _ = exec.tx.send(Event::SttFailed { session_id, error: e });
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
                let registry = self.registry.clone();
                let lang = self.settings.get().dictation.language.clone();
                let tx = exec.tx.clone();
                tokio::spawn(async move {
                    let stt = match registry.stt_for(SlotKind::Stt) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = tx.send(Event::SttFailed { session_id, error: e });
                            return;
                        }
                    };
                    let result = crate::providers::stt::transcribe_auto_chunk(
                        stt.as_ref(),
                        AudioInput { wav_16k_mono: rec.wav_16k_mono, duration_ms: rec.duration_ms },
                        SttOptions { language: Some(lang), prompt: None, temperature: None },
                    )
                    .await;
                    let event = match result {
                        Ok(t) if t.text.trim().is_empty() => Event::SttFailed {
                            session_id,
                            error: TypexError::new(ErrorCode::NoSpeech, "没有听到声音"),
                        },
                        Ok(t) => Event::SttResult { session_id, transcript: t.text.trim().to_string() },
                        Err(e) => Event::SttFailed { session_id, error: e.into() },
                    };
                    let _ = tx.send(event);
                });
            }
            Effect::CallProcess { session_id, mode, transcript } => {
                // 助手模式：转写结果 = 语音指令 → 交给助手面板（F-3），主会话结束
                if mode == SessionMode::Assistant {
                    if let Some(assistant) = &self.assistant {
                        let selection = self.pending_selection.lock().unwrap().take();
                        match assistant.ask(transcript, selection) {
                            Ok(_) => {
                                let _ = exec.tx.send(Event::AssistantHandedOff { session_id });
                                (self.show_assistant_panel)();
                            }
                            Err(e) => {
                                let _ = exec.tx.send(Event::ProcessFailed { session_id, error: e });
                            }
                        }
                    } else {
                        let _ = exec.tx.send(Event::ProcessFailed {
                            session_id,
                            error: TypexError::new(ErrorCode::NotConfigured, "助手服务未装配"),
                        });
                    }
                    return;
                }
                let tx = exec.tx.clone();
                let settings = self.settings.get();
                let registry = self.registry.clone();
                tokio::spawn(async move {
                    let event =
                        match pipeline::process(mode, transcript, &settings, &registry).await {
                            pipeline::ProcessOutcome::Done(text) => {
                                Event::ProcessResult { session_id, text }
                            }
                            pipeline::ProcessOutcome::Degraded(original) => {
                                Event::ProcessDegraded { session_id, original }
                            }
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
                *self.last_result.lock().unwrap() = Some(text.clone());
                // enigo/剪贴板是阻塞调用 → blocking 线程
                tokio::task::spawn_blocking(move || {
                    let event = match injector.inject(&text) {
                        Ok(()) => Event::InjectDone { session_id },
                        Err(e) => Event::InjectFailed { session_id, error: e },
                    };
                    let _ = tx.send(event);
                });
            }
            Effect::EmitUi => self.emit_snapshot(exec),
            Effect::EmitBusyHint => {
                // CP-1.3：HUD 轻晃 + 「正在处理上一条…」；快照层先透传 phase
                self.emit_snapshot(exec);
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
                tokio::task::spawn_blocking(move || {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(text);
                    }
                });
            }
            Effect::ReleaseAudio { session_id } => {
                exec.audio_store.remove(&session_id);
            }
            Effect::ShowAssistantPanel => {
                (self.show_assistant_panel)();
            }
        }
    }

    fn emit_snapshot(&self, exec: &Exec) {
        let mut snap = snapshot_of(&exec.state, exec.recording_started);
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
            snap.recording_ms =
                recording_started.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0);
        }
        State::Processing { .. } => {
            snap.processing_step = Some("processing".into());
            snap.has_transcript = true;
        }
        State::Injecting { unpolished, .. } => {
            snap.unpolished = *unpolished;
            snap.has_transcript = true;
        }
        State::Failed { error, stage, transcript, .. } => {
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
