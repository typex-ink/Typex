//! 会话状态机（07 §5.2）：纯函数式转移表，不做 IO。
//!
//! `advance(state, event) -> (state, Vec<Effect>)` — Effect 由 orchestrator
//! 执行器 dispatch 到各 service。全项目单测密度最高处（08 §3.1 场景清单）。

use crate::error::{ErrorCode, TypexError};
use crate::types::{FailedStage, SessionMode, SessionPhase};

/// 长按/短按判定阈值（可被设置覆盖）。
pub const DEFAULT_HOLD_THRESHOLD_MS: u64 = 350;

/// 状态机内部状态（携带 payload；对前端的投影见 SessionSnapshot）。
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Idle,
    Recording {
        session_id: u64,
        mode: SessionMode,
        /// toggle 模式（短按开始，再按结束）；押住模式为 false
        toggled: bool,
    },
    Transcribing {
        session_id: u64,
        mode: SessionMode,
    },
    /// F-1 整理 / F-2 翻译 / F-3 LLM 调用
    Processing {
        session_id: u64,
        mode: SessionMode,
        transcript: String,
    },
    Injecting {
        session_id: u64,
        mode: SessionMode,
        text: String,
        /// 整理失败降级注入原文（HUD「未整理」）
        unpolished: bool,
    },
    Failed {
        session_id: u64,
        mode: SessionMode,
        stage: FailedStage,
        error: TypexError,
        /// 保住已有产物（不丢话铁律）：Transcribing 失败 = 音频可重试；
        /// Processing 失败 = 转写稿可复制/注入原文
        transcript: Option<String>,
    },
}

impl State {
    pub fn phase(&self) -> SessionPhase {
        match self {
            State::Idle => SessionPhase::Idle,
            State::Recording { .. } => SessionPhase::Recording,
            State::Transcribing { .. } => SessionPhase::Transcribing,
            State::Processing { .. } => SessionPhase::Processing,
            State::Injecting { .. } => SessionPhase::Injecting,
            State::Failed { .. } => SessionPhase::Failed,
        }
    }

    pub fn session_id(&self) -> Option<u64> {
        match self {
            State::Idle => None,
            State::Recording { session_id, .. }
            | State::Transcribing { session_id, .. }
            | State::Processing { session_id, .. }
            | State::Injecting { session_id, .. }
            | State::Failed { session_id, .. } => Some(*session_id),
        }
    }

    pub fn mode(&self) -> Option<SessionMode> {
        match self {
            State::Idle => None,
            State::Recording { mode, .. }
            | State::Transcribing { mode, .. }
            | State::Processing { mode, .. }
            | State::Injecting { mode, .. }
            | State::Failed { mode, .. } => Some(*mode),
        }
    }
}

/// 输入事件（来自 hotkey 线程、service 异步回调、HUD 按钮）。
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// 触发键按下（乐观启动）。next_session_id 由 orchestrator 预分配。
    TriggerDown { mode: SessionMode, next_session_id: u64 },
    /// 按住期间组合出翻译键
    ModeUpgraded { mode: SessionMode },
    /// 全部触发键松开
    TriggerUp { held_ms: u64 },
    /// 组合键让路（普通键介入）
    Yielded,
    /// Esc（listen-only；仅 Recording 响应）
    Esc,
    /// HUD ✕ / dismiss
    Dismiss,
    /// HUD 重试按钮
    Retry,
    /// HUD「复制原文」（STT 已成功、后续失败时；不丢话铁律）
    CopyTranscriptRequested,
    /// HUD「注入原文」（翻译失败降级，02 F-2）
    InjectOriginalRequested,
    /// 录音服务完成（携带产物就绪信号；音频本体在 orchestrator 手里）
    RecordingFinished { session_id: u64 },
    /// STT 成功
    SttResult { session_id: u64, transcript: String },
    /// STT 失败
    SttFailed { session_id: u64, error: TypexError },
    /// 处理（整理/翻译/问答）成功
    ProcessResult { session_id: u64, text: String },
    /// 处理失败
    ProcessFailed { session_id: u64, error: TypexError },
    /// 整理层失败但按降级策略直通原文（F-9：绝不阻塞主流程）
    ProcessDegraded { session_id: u64, original: String },
    /// 注入完成
    InjectDone { session_id: u64 },
    /// 注入失败
    InjectFailed { session_id: u64, error: TypexError },
}

/// 副作用指令：由执行器 dispatch，状态机本体零 IO。
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    StartRecording,
    StopRecording { session_id: u64 },
    CancelRecording,
    /// 停止录音并以音频调 STT
    CallStt { session_id: u64 },
    /// 以转写稿调处理层（整理/翻译/问答，按 mode 选流水线）
    CallProcess { session_id: u64, mode: SessionMode, transcript: String },
    Inject { session_id: u64, text: String },
    /// 推送 SessionSnapshot 给前端
    EmitUi,
    /// 忙碌提示（重按忽略时 HUD 轻晃 + 微文案）
    EmitBusyHint,
    PlayChime(Chime),
    /// 复制文本到剪贴板（失败兜底「复制原文」等）
    CopyToClipboard(String),
    /// 会话彻底结束，释放临时音频
    ReleaseAudio { session_id: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chime {
    Start,
    Success,
    Error,
}

/// 判定短按（toggle）还是长按（push-to-talk）。
fn is_toggle(held_ms: u64, threshold_ms: u64) -> bool {
    held_ms < threshold_ms
}

/// 状态转移函数。`threshold_ms`：长短按阈值（设置注入）。
pub fn advance(state: State, event: Event, threshold_ms: u64) -> (State, Vec<Effect>) {
    use Effect as E;
    match (state, event) {
        // ───────── Idle ─────────
        (State::Idle, Event::TriggerDown { mode, next_session_id }) => (
            State::Recording { session_id: next_session_id, mode, toggled: false },
            vec![E::StartRecording, E::EmitUi, E::PlayChime(Chime::Start)],
        ),
        (s @ State::Idle, _) => (s, vec![]),

        // ───────── Recording ─────────
        (State::Recording { session_id, .. }, Event::ModeUpgraded { mode }) => (
            // 模式升级（听写→翻译）：音频保留，仅换徽标
            State::Recording { session_id, mode, toggled: false },
            vec![E::EmitUi],
        ),
        (State::Recording { session_id, mode, toggled }, Event::TriggerUp { held_ms }) => {
            if !toggled && is_toggle(held_ms, threshold_ms) {
                // 短按 = toggle 开始：继续录音，等待第二次按键
                (State::Recording { session_id, mode, toggled: true }, vec![E::EmitUi])
            } else {
                // push-to-talk 结束（或 toggle 后的松开——不可能路径，防御性同样结束）
                (
                    State::Transcribing { session_id, mode },
                    vec![E::CallStt { session_id }, E::EmitUi],
                )
            }
        }
        (State::Recording { session_id, mode, toggled: true }, Event::TriggerDown { .. }) => {
            // toggle 模式下二次按下 = 结束录音
            (
                State::Transcribing { session_id, mode },
                vec![E::CallStt { session_id }, E::EmitUi],
            )
        }
        (s @ State::Recording { toggled: false, .. }, Event::TriggerDown { .. }) => {
            // 按住期间的重复 down（OS 重复已在 detector 滤掉；防御）
            (s, vec![])
        }
        (State::Recording { session_id, .. }, Event::Yielded) => {
            // 组合键让路：静默取消，无任何注入/提示音（08 §3.1）
            (State::Idle, vec![E::CancelRecording, E::ReleaseAudio { session_id }])
        }
        (State::Recording { session_id, .. }, Event::Esc | Event::Dismiss) => (
            State::Idle,
            vec![E::CancelRecording, E::EmitUi, E::ReleaseAudio { session_id }],
        ),
        (s @ State::Recording { .. }, _) => (s, vec![]),

        // ───────── Transcribing ─────────
        (State::Transcribing { session_id, mode }, Event::SttResult { session_id: sid, transcript })
            if sid == session_id =>
        {
            (
                State::Processing { session_id, mode, transcript: transcript.clone() },
                vec![E::CallProcess { session_id, mode, transcript }, E::EmitUi],
            )
        }
        (State::Transcribing { session_id, mode }, Event::SttFailed { session_id: sid, error })
            if sid == session_id =>
        {
            (
                State::Failed {
                    session_id,
                    mode,
                    stage: FailedStage::Transcribing,
                    error,
                    transcript: None, // 音频保留在 orchestrator（可重试）
                },
                vec![E::EmitUi, E::PlayChime(Chime::Error)],
            )
        }
        // 忙碌重按：状态不变 + 提示（Transcribing/Processing/Injecting 共通）
        (s @ State::Transcribing { .. }, Event::TriggerDown { .. }) => (s, vec![E::EmitBusyHint]),
        (s @ State::Transcribing { .. }, _) => (s, vec![]),

        // ───────── Processing ─────────
        (State::Processing { session_id, mode, .. }, Event::ProcessResult { session_id: sid, text })
            if sid == session_id =>
        {
            (
                State::Injecting { session_id, mode, text: text.clone(), unpolished: false },
                vec![E::Inject { session_id, text }, E::EmitUi],
            )
        }
        (
            State::Processing { session_id, mode, .. },
            Event::ProcessDegraded { session_id: sid, original },
        ) if sid == session_id => {
            // F-9 降级：注入原始转写 + HUD「未整理」标注
            (
                State::Injecting { session_id, mode, text: original.clone(), unpolished: true },
                vec![E::Inject { session_id, text: original }, E::EmitUi],
            )
        }
        (
            State::Processing { session_id, mode, transcript },
            Event::ProcessFailed { session_id: sid, error },
        ) if sid == session_id => {
            // 转写稿保留：HUD 提供「复制原文」/（翻译模式）「注入原文」
            (
                State::Failed {
                    session_id,
                    mode,
                    stage: FailedStage::Processing,
                    error,
                    transcript: Some(transcript),
                },
                vec![E::EmitUi, E::PlayChime(Chime::Error)],
            )
        }
        (s @ State::Processing { .. }, Event::TriggerDown { .. }) => (s, vec![E::EmitBusyHint]),
        (s @ State::Processing { .. }, _) => (s, vec![]),

        // ───────── Injecting ─────────
        (State::Injecting { session_id, .. }, Event::InjectDone { session_id: sid })
            if sid == session_id =>
        {
            (
                State::Idle,
                vec![E::EmitUi, E::PlayChime(Chime::Success), E::ReleaseAudio { session_id }],
            )
        }
        (
            State::Injecting { session_id, mode, text, .. },
            Event::InjectFailed { session_id: sid, error },
        ) if sid == session_id => {
            // 注入失败 → 自动转剪贴板 + 明示（05 §9 兜底）
            let effects = if error.code == ErrorCode::NoFocus {
                vec![E::CopyToClipboard(text.clone()), E::EmitUi]
            } else {
                vec![E::CopyToClipboard(text.clone()), E::EmitUi, E::PlayChime(Chime::Error)]
            };
            (
                State::Failed {
                    session_id,
                    mode,
                    stage: FailedStage::Injecting,
                    error,
                    transcript: Some(text),
                },
                effects,
            )
        }
        (s @ State::Injecting { .. }, Event::TriggerDown { .. }) => (s, vec![E::EmitBusyHint]),
        (s @ State::Injecting { .. }, _) => (s, vec![]),

        // ───────── Failed ─────────
        (State::Failed { session_id, mode, stage, transcript, .. }, Event::Retry) => {
            // 从失败的 stage 恢复，而非从头（08 §3.1）
            match stage {
                FailedStage::Transcribing => (
                    State::Transcribing { session_id, mode },
                    vec![E::CallStt { session_id }, E::EmitUi],
                ),
                FailedStage::Processing => {
                    let t = transcript.unwrap_or_default();
                    (
                        State::Processing { session_id, mode, transcript: t.clone() },
                        vec![E::CallProcess { session_id, mode, transcript: t }, E::EmitUi],
                    )
                }
                FailedStage::Injecting | FailedStage::Recording => {
                    let t = transcript.unwrap_or_default();
                    (
                        State::Injecting { session_id, mode, text: t.clone(), unpolished: false },
                        vec![E::Inject { session_id, text: t }, E::EmitUi],
                    )
                }
            }
        }
        (State::Failed { session_id, .. }, Event::TriggerDown { mode, next_session_id }) => {
            // 失败态按触发键 = 放弃旧会话开新录音（08 §3.1）
            (
                State::Recording { session_id: next_session_id, mode, toggled: false },
                vec![
                    E::ReleaseAudio { session_id },
                    E::StartRecording,
                    E::EmitUi,
                    E::PlayChime(Chime::Start),
                ],
            )
        }
        (State::Failed { session_id, .. }, Event::Dismiss) => (
            State::Idle,
            vec![E::EmitUi, E::ReleaseAudio { session_id }],
        ),
        (s @ State::Failed { transcript: Some(_), .. }, Event::CopyTranscriptRequested) => {
            let t = match &s {
                State::Failed { transcript: Some(t), .. } => t.clone(),
                _ => unreachable!(),
            };
            (s, vec![E::CopyToClipboard(t)])
        }
        (
            State::Failed { session_id, mode, transcript: Some(t), .. },
            Event::InjectOriginalRequested,
        ) => (
            State::Injecting { session_id, mode, text: t.clone(), unpolished: true },
            vec![E::Inject { session_id, text: t }, E::EmitUi],
        ),
        (s @ State::Failed { .. }, _) => (s, vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = DEFAULT_HOLD_THRESHOLD_MS;

    fn err(code: ErrorCode) -> TypexError {
        TypexError::new(code, "test")
    }

    fn recording(id: u64) -> State {
        State::Recording { session_id: id, mode: SessionMode::Dictation, toggled: false }
    }

    fn down(id: u64) -> Event {
        Event::TriggerDown { mode: SessionMode::Dictation, next_session_id: id }
    }

    // ── 长按/短按（08 §3.1 场景 1）──

    #[test]
    fn hold_349ms_release_enters_toggle_mode() {
        let (s, fx) = advance(recording(1), Event::TriggerUp { held_ms: 349 }, T);
        assert_eq!(s, State::Recording { session_id: 1, mode: SessionMode::Dictation, toggled: true });
        assert!(!fx.contains(&Effect::CallStt { session_id: 1 }));
    }

    #[test]
    fn hold_351ms_release_is_push_to_talk_end() {
        let (s, fx) = advance(recording(1), Event::TriggerUp { held_ms: 351 }, T);
        assert_eq!(s.phase(), SessionPhase::Transcribing);
        assert!(fx.contains(&Effect::CallStt { session_id: 1 }));
    }

    #[test]
    fn toggle_second_press_ends_recording() {
        let (s, _) = advance(recording(1), Event::TriggerUp { held_ms: 100 }, T);
        let (s, fx) = advance(s, down(99), T);
        assert_eq!(s.phase(), SessionPhase::Transcribing);
        assert_eq!(s.session_id(), Some(1)); // 沿用原会话，不开新会话
        assert!(fx.contains(&Effect::CallStt { session_id: 1 }));
    }

    // ── 组合键（场景 2）──

    #[test]
    fn combo_upgrades_mode_and_keeps_audio() {
        let (s, fx) =
            advance(recording(1), Event::ModeUpgraded { mode: SessionMode::Translation }, T);
        assert_eq!(s.mode(), Some(SessionMode::Translation));
        assert_eq!(s.session_id(), Some(1));
        // 无 CancelRecording/StartRecording —— 音频保留
        assert!(!fx.contains(&Effect::CancelRecording));
        assert!(!fx.contains(&Effect::StartRecording));
    }

    // ── 组合键让路（场景 3）──

    #[test]
    fn yield_cancels_silently() {
        let (s, fx) = advance(recording(1), Event::Yielded, T);
        assert_eq!(s, State::Idle);
        assert!(fx.contains(&Effect::CancelRecording));
        // 静默：无提示音、无注入、无 UI 弹动（EmitUi 也不发——HUD 直接隐藏由 CancelRecording 携带）
        assert!(!fx.iter().any(|e| matches!(e, Effect::PlayChime(_))));
        assert!(!fx.iter().any(|e| matches!(e, Effect::Inject { .. })));
    }

    // ── 重按忽略（场景 4）──

    #[test]
    fn press_during_transcribing_is_ignored_with_busy_hint() {
        let s0 = State::Transcribing { session_id: 1, mode: SessionMode::Dictation };
        let (s, fx) = advance(s0.clone(), down(2), T);
        assert_eq!(s, s0);
        assert_eq!(fx, vec![Effect::EmitBusyHint]);
    }

    #[test]
    fn press_during_processing_and_injecting_ignored() {
        let p = State::Processing { session_id: 1, mode: SessionMode::Dictation, transcript: "x".into() };
        let (s, fx) = advance(p.clone(), down(2), T);
        assert_eq!(s, p);
        assert_eq!(fx, vec![Effect::EmitBusyHint]);

        let i = State::Injecting { session_id: 1, mode: SessionMode::Dictation, text: "x".into(), unpolished: false };
        let (s, fx) = advance(i.clone(), down(2), T);
        assert_eq!(s, i);
        assert_eq!(fx, vec![Effect::EmitBusyHint]);
    }

    #[test]
    fn press_in_failed_state_abandons_and_starts_new() {
        let f = State::Failed {
            session_id: 1,
            mode: SessionMode::Dictation,
            stage: FailedStage::Transcribing,
            error: err(ErrorCode::NetworkError),
            transcript: None,
        };
        let (s, fx) = advance(f, down(2), T);
        assert_eq!(s.phase(), SessionPhase::Recording);
        assert_eq!(s.session_id(), Some(2));
        assert!(fx.contains(&Effect::ReleaseAudio { session_id: 1 }));
        assert!(fx.contains(&Effect::StartRecording));
    }

    // ── Esc（场景 5）──

    #[test]
    fn esc_only_cancels_recording_state() {
        let (s, fx) = advance(recording(1), Event::Esc, T);
        assert_eq!(s, State::Idle);
        assert!(fx.contains(&Effect::CancelRecording));

        // 其他态 Esc 无效果
        for st in [
            State::Transcribing { session_id: 1, mode: SessionMode::Dictation },
            State::Processing { session_id: 1, mode: SessionMode::Dictation, transcript: "x".into() },
            State::Idle,
        ] {
            let (s2, fx2) = advance(st.clone(), Event::Esc, T);
            assert_eq!(s2, st);
            assert!(fx2.is_empty());
        }
    }

    // ── 失败恢复（场景 6）──

    #[test]
    fn stt_failure_keeps_audio_recoverable() {
        let s0 = State::Transcribing { session_id: 1, mode: SessionMode::Dictation };
        let (s, fx) = advance(
            s0,
            Event::SttFailed { session_id: 1, error: err(ErrorCode::NetworkError) },
            T,
        );
        match &s {
            State::Failed { stage, transcript, .. } => {
                assert_eq!(*stage, FailedStage::Transcribing);
                assert!(transcript.is_none()); // 音频在 orchestrator，转写稿尚无
            }
            _ => panic!("expected Failed"),
        }
        // 无 ReleaseAudio —— 音频保留待重试
        assert!(!fx.iter().any(|e| matches!(e, Effect::ReleaseAudio { .. })));
    }

    #[test]
    fn process_failure_keeps_transcript_for_copy() {
        let s0 = State::Processing { session_id: 1, mode: SessionMode::Dictation, transcript: "原文".into() };
        let (s, _) = advance(
            s0,
            Event::ProcessFailed { session_id: 1, error: err(ErrorCode::Timeout) },
            T,
        );
        match &s {
            State::Failed { stage, transcript, .. } => {
                assert_eq!(*stage, FailedStage::Processing);
                assert_eq!(transcript.as_deref(), Some("原文"));
            }
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn retry_resumes_from_failed_stage_not_from_start() {
        // Transcribing 失败重试 → 回 Transcribing（重发 STT）
        let f1 = State::Failed {
            session_id: 1,
            mode: SessionMode::Dictation,
            stage: FailedStage::Transcribing,
            error: err(ErrorCode::NetworkError),
            transcript: None,
        };
        let (s, fx) = advance(f1, Event::Retry, T);
        assert_eq!(s.phase(), SessionPhase::Transcribing);
        assert!(fx.contains(&Effect::CallStt { session_id: 1 }));

        // Processing 失败重试 → 回 Processing（带原转写稿，不重录不重转）
        let f2 = State::Failed {
            session_id: 1,
            mode: SessionMode::Translation,
            stage: FailedStage::Processing,
            error: err(ErrorCode::Timeout),
            transcript: Some("原文".into()),
        };
        let (s, fx) = advance(f2, Event::Retry, T);
        assert_eq!(s.phase(), SessionPhase::Processing);
        assert!(fx.contains(&Effect::CallProcess {
            session_id: 1,
            mode: SessionMode::Translation,
            transcript: "原文".into()
        }));
    }

    // ── session_id 竞态（场景 7：防「上一句注入到下一句」核心测试）──

    #[test]
    fn stale_stt_result_is_dropped() {
        let s0 = State::Transcribing { session_id: 2, mode: SessionMode::Dictation };
        let (s, fx) = advance(
            s0.clone(),
            Event::SttResult { session_id: 1, transcript: "旧会话结果".into() },
            T,
        );
        assert_eq!(s, s0); // 状态零变化
        assert!(fx.is_empty());
    }

    #[test]
    fn stale_process_result_is_dropped() {
        let s0 = State::Processing { session_id: 3, mode: SessionMode::Dictation, transcript: "x".into() };
        let (s, fx) = advance(s0.clone(), Event::ProcessResult { session_id: 2, text: "旧".into() }, T);
        assert_eq!(s, s0);
        assert!(fx.is_empty());
    }

    #[test]
    fn callback_after_cancel_is_dropped() {
        // Recording 中取消 → Idle；迟到的 SttResult 不得产生任何效果
        let (s, _) = advance(recording(1), Event::Esc, T);
        assert_eq!(s, State::Idle);
        let (s, fx) = advance(s, Event::SttResult { session_id: 1, transcript: "迟到".into() }, T);
        assert_eq!(s, State::Idle);
        assert!(fx.is_empty());
    }

    // ── 整理层降级（场景 8）──

    #[test]
    fn polish_degradation_injects_original_with_unpolished_flag() {
        let s0 = State::Processing { session_id: 1, mode: SessionMode::Dictation, transcript: "嗯原文".into() };
        let (s, fx) = advance(
            s0,
            Event::ProcessDegraded { session_id: 1, original: "嗯原文".into() },
            T,
        );
        match &s {
            State::Injecting { unpolished, text, .. } => {
                assert!(*unpolished);
                assert_eq!(text, "嗯原文");
            }
            _ => panic!("expected Injecting"),
        }
        assert!(fx.contains(&Effect::Inject { session_id: 1, text: "嗯原文".into() }));
    }

    // ── 翻译降级（场景 9）──

    #[test]
    fn translation_failure_offers_transcript_via_failed_state() {
        let s0 = State::Processing {
            session_id: 1,
            mode: SessionMode::Translation,
            transcript: "中文原文".into(),
        };
        let (s, _) = advance(
            s0,
            Event::ProcessFailed { session_id: 1, error: err(ErrorCode::ServerError) },
            T,
        );
        // Failed 且转写稿保留 → HUD 可提供「注入原文」（executor 层将其映射为按钮）
        match &s {
            State::Failed { mode, transcript, .. } => {
                assert_eq!(*mode, SessionMode::Translation);
                assert_eq!(transcript.as_deref(), Some("中文原文"));
            }
            _ => panic!("expected Failed"),
        }
    }

    // ── 完整成功路径 ──

    #[test]
    fn happy_path_dictation() {
        let (s, fx) = advance(State::Idle, down(1), T);
        assert_eq!(s.phase(), SessionPhase::Recording);
        assert!(fx.contains(&Effect::StartRecording));
        assert!(fx.contains(&Effect::PlayChime(Chime::Start)));

        let (s, _) = advance(s, Event::TriggerUp { held_ms: 2000 }, T);
        assert_eq!(s.phase(), SessionPhase::Transcribing);

        let (s, fx) = advance(s, Event::SttResult { session_id: 1, transcript: "嗯你好".into() }, T);
        assert_eq!(s.phase(), SessionPhase::Processing);
        assert!(fx.contains(&Effect::CallProcess {
            session_id: 1,
            mode: SessionMode::Dictation,
            transcript: "嗯你好".into()
        }));

        let (s, fx) = advance(s, Event::ProcessResult { session_id: 1, text: "你好".into() }, T);
        assert_eq!(s.phase(), SessionPhase::Injecting);
        assert!(fx.contains(&Effect::Inject { session_id: 1, text: "你好".into() }));

        let (s, fx) = advance(s, Event::InjectDone { session_id: 1 }, T);
        assert_eq!(s, State::Idle);
        assert!(fx.contains(&Effect::PlayChime(Chime::Success)));
        assert!(fx.contains(&Effect::ReleaseAudio { session_id: 1 }));
    }

    // ── 注入失败兜底 ──

    #[test]
    fn inject_failure_copies_to_clipboard() {
        let s0 = State::Injecting { session_id: 1, mode: SessionMode::Dictation, text: "结果".into(), unpolished: false };
        let (s, fx) = advance(
            s0,
            Event::InjectFailed { session_id: 1, error: err(ErrorCode::NoFocus) },
            T,
        );
        assert_eq!(s.phase(), SessionPhase::Failed);
        assert!(fx.contains(&Effect::CopyToClipboard("结果".into())));
        // NoFocus 是常见情况，不放错误音
        assert!(!fx.iter().any(|e| matches!(e, Effect::PlayChime(Chime::Error))));
    }

    #[test]
    fn dismiss_failed_releases_audio() {
        let f = State::Failed {
            session_id: 7,
            mode: SessionMode::Dictation,
            stage: FailedStage::Transcribing,
            error: err(ErrorCode::NetworkError),
            transcript: None,
        };
        let (s, fx) = advance(f, Event::Dismiss, T);
        assert_eq!(s, State::Idle);
        assert!(fx.contains(&Effect::ReleaseAudio { session_id: 7 }));
    }
}
