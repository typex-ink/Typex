//! 全局快捷键：trait HotkeyBackend + 纯逻辑判定器（06 §7.3）。
//!
//! 平台监听线程只做「原始键事件 → Detector 判定 → mpsc 发送语义事件」；
//! 长按/短按与会话语义由 orchestrator 状态机处理，本层只负责：
//! - 触发键识别（含右⌘+右⌥ 组合升级为翻译）
//! - 组合键让路（触发键按住期间出现普通键 → Yield）
//! - Esc 会话认领与物理序列选择性吞键

#[cfg(not(target_os = "windows"))]
pub mod rdev_backend;
#[cfg(target_os = "windows")]
pub mod windows_backend;

#[cfg(target_os = "windows")]
pub struct ManagedWindowsHotkey {
    handle: Option<windows_backend::WindowsHotkeyHandle>,
    health_rx: tokio::sync::watch::Receiver<windows_backend::WindowsHookHealth>,
}

#[cfg(target_os = "windows")]
impl ManagedWindowsHotkey {
    pub fn running(handle: windows_backend::WindowsHotkeyHandle) -> Self {
        let health_rx = handle.subscribe_health();
        Self {
            handle: Some(handle),
            health_rx,
        }
    }

    pub fn failed(error: windows_backend::WindowsHookError) -> Self {
        let (_health_tx, health_rx) =
            tokio::sync::watch::channel(windows_backend::WindowsHookHealth::Failed(error));
        Self {
            handle: None,
            health_rx,
        }
    }

    pub fn health(&self) -> windows_backend::WindowsHookHealth {
        self.health_rx.borrow().clone()
    }

    pub fn subscribe_health(
        &self,
    ) -> tokio::sync::watch::Receiver<windows_backend::WindowsHookHealth> {
        self.health_rx.clone()
    }

    pub fn shutdown(&self) -> Result<(), windows_backend::WindowsHookError> {
        self.handle
            .as_ref()
            .map_or(Ok(()), windows_backend::WindowsHotkeyHandle::shutdown)
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct WindowsHookHealthAction {
    pub cancel_session: bool,
    pub refresh_status: bool,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Default)]
pub(crate) struct WindowsHookFailureLatch {
    terminal_seen: bool,
}

#[cfg(target_os = "windows")]
impl WindowsHookFailureLatch {
    pub fn observe(
        &mut self,
        health: &windows_backend::WindowsHookHealth,
        paused: bool,
    ) -> WindowsHookHealthAction {
        if self.terminal_seen || !health.is_unexpected_terminal() {
            return WindowsHookHealthAction::default();
        }

        self.terminal_seen = true;
        WindowsHookHealthAction {
            cancel_session: !paused,
            refresh_status: true,
        }
    }
}

use crate::types::{
    KeyId, SessionMode, canonical_key_id, normalize_hotkey_chord, supports_stale_release_recovery,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 修饰键正常不会自动连发；同一触发键在这个窗口后再次 down，
/// 视为上一轮 release 丢失，重置判定器以恢复下一次触发。
const STALE_DUPLICATE_DOWN_MS: u64 = 250;

/// 判定器输出的语义事件（发往 orchestrator）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// 已确认的触发键按下（Windows 默认右侧修饰键由 adapter 确认后发出）
    TriggerDown { mode: SessionMode },
    /// Windows raw modifier down immediately starts an invisible audio candidate.
    CaptureCandidateStarted { token: u64 },
    /// The delayed TriggerDown confirms and promotes the matching candidate.
    CaptureCandidatePromoted { token: u64, mode: SessionMode },
    /// A normal chord or runtime reset discards the matching candidate silently.
    CaptureCandidateCancelled { token: u64 },
    /// 按住期间组合出另一触发键 → 升级为翻译（音频保留）
    ModeUpgraded { mode: SessionMode },
    /// 全部触发键松开；held_ms 自首个触发键按下起算
    TriggerUp { held_ms: u64 },
    /// 组合键让路：触发键按住期间出现普通键 → 静默取消
    Yielded,
    /// Esc 已同步认领指定会话；执行器仍须校验该 ID 是否新鲜。
    EscPressed { session_id: u64 },
}

pub(crate) type InjectionCancel = Arc<dyn Fn() -> bool + Send + Sync>;

enum EscCancellationState {
    Idle,
    Cancellable {
        session_id: u64,
    },
    Injecting {
        session_id: u64,
        cancel_before_commit: InjectionCancel,
    },
    Claimed {
        session_id: u64,
    },
}

/// Thread-safe ownership gate used by platform hooks before consuming Escape.
pub struct EscCancellationLatch {
    enabled: AtomicBool,
    state: Mutex<EscCancellationState>,
}

impl EscCancellationLatch {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            state: Mutex::new(EscCancellationState::Idle),
        }
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn arm_cancellable(&self, session_id: u64) {
        let mut state = self.state.lock().unwrap();
        if matches!(
            *state,
            EscCancellationState::Claimed {
                session_id: claimed
            } if claimed == session_id
        ) {
            return;
        }
        *state = EscCancellationState::Cancellable { session_id };
    }

    #[cfg(test)]
    pub(crate) fn arm_injecting(&self, session_id: u64, cancel_before_commit: InjectionCancel) {
        let mut state = self.state.lock().unwrap();
        if matches!(
            *state,
            EscCancellationState::Claimed {
                session_id: claimed
            } if claimed == session_id
        ) {
            let _ = cancel_before_commit();
            return;
        }
        *state = EscCancellationState::Injecting {
            session_id,
            cancel_before_commit,
        };
    }

    pub(crate) fn disarm(&self) {
        *self.state.lock().unwrap() = EscCancellationState::Idle;
    }

    fn replace_if_unclaimed(
        &self,
        previous_session_id: Option<u64>,
        next: EscCancellationState,
    ) -> bool {
        let mut state = self.state.lock().unwrap();
        if matches!(
            *state,
            EscCancellationState::Claimed { session_id }
                if previous_session_id == Some(session_id)
        ) {
            return false;
        }
        *state = next;
        true
    }

    pub(crate) fn transition_to_idle(&self, previous_session_id: Option<u64>) -> bool {
        self.replace_if_unclaimed(previous_session_id, EscCancellationState::Idle)
    }

    pub(crate) fn transition_to_cancellable(
        &self,
        previous_session_id: Option<u64>,
        session_id: u64,
    ) -> bool {
        self.replace_if_unclaimed(
            previous_session_id,
            EscCancellationState::Cancellable { session_id },
        )
    }

    pub(crate) fn transition_to_injecting(
        &self,
        previous_session_id: Option<u64>,
        session_id: u64,
        cancel_before_commit: InjectionCancel,
    ) -> bool {
        self.replace_if_unclaimed(
            previous_session_id,
            EscCancellationState::Injecting {
                session_id,
                cancel_before_commit,
            },
        )
    }

    pub(crate) fn try_claim(&self) -> Option<u64> {
        if !self.enabled.load(Ordering::Acquire) {
            return None;
        }
        let mut state = self.state.lock().unwrap();
        if !self.enabled.load(Ordering::Acquire) {
            return None;
        }
        let session_id = match &*state {
            EscCancellationState::Cancellable { session_id } => *session_id,
            EscCancellationState::Injecting {
                session_id,
                cancel_before_commit,
            } if cancel_before_commit() => *session_id,
            EscCancellationState::Idle
            | EscCancellationState::Injecting { .. }
            | EscCancellationState::Claimed { .. } => return None,
        };
        *state = EscCancellationState::Claimed { session_id };
        Some(session_id)
    }
}

impl Default for EscCancellationLatch {
    fn default() -> Self {
        Self::new(true)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct EscKeyDecision {
    pub event: Option<HotkeyEvent>,
    pub swallow: bool,
}

/// Tracks one physical Escape sequence so repeats never claim a later session
/// and a consumed down always has a consumed paired up.
#[derive(Debug, Default)]
pub(crate) struct EscKeyFilter {
    pressed: bool,
    swallowing: bool,
}

impl EscKeyFilter {
    pub(crate) fn on_key(
        &mut self,
        down: bool,
        allow_claim: bool,
        latch: &EscCancellationLatch,
    ) -> EscKeyDecision {
        if down {
            if self.pressed {
                return EscKeyDecision {
                    event: None,
                    swallow: self.swallowing,
                };
            }
            self.pressed = true;
            if allow_claim && let Some(session_id) = latch.try_claim() {
                self.swallowing = true;
                return EscKeyDecision {
                    event: Some(HotkeyEvent::EscPressed { session_id }),
                    swallow: true,
                };
            }
            return EscKeyDecision::default();
        }

        if !self.pressed {
            return EscKeyDecision::default();
        }
        self.pressed = false;
        let swallow = std::mem::take(&mut self.swallowing);
        EscKeyDecision {
            event: None,
            swallow,
        }
    }
}

/// 判定器配置：各功能的触发键组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyConfig {
    pub dictation: Vec<KeyId>,
    pub assistant: Vec<KeyId>,
    /// 独立翻译 chord（默认仍为 dictation + assistant 各一键）
    pub translation: Vec<KeyId>,
    /// Escape participates in session cancellation instead of ordinary-key yielding.
    pub esc_cancels: bool,
}

impl HotkeyConfig {
    pub fn from_settings(settings: &crate::settings::schema::Settings) -> Self {
        let h = &settings.hotkeys;
        Self {
            dictation: h.dictation.clone(),
            assistant: h.assistant.clone(),
            translation: h.translation.clone(),
            esc_cancels: settings.dictation.esc_cancels,
        }
        .normalized()
    }

    fn normalized(mut self) -> Self {
        self.dictation = normalize_hotkey_chord(&self.dictation);
        self.assistant = normalize_hotkey_chord(&self.assistant);
        self.translation = normalize_hotkey_chord(&self.translation);
        self
    }

    fn is_trigger_key(&self, key: &str) -> bool {
        self.dictation.iter().any(|candidate| candidate == key)
            || self.assistant.iter().any(|candidate| candidate == key)
            || self.translation.iter().any(|candidate| candidate == key)
    }

    fn same_chords(&self, other: &Self) -> bool {
        self.dictation == other.dictation
            && self.assistant == other.assistant
            && self.translation == other.translation
    }
}

#[derive(Debug, Clone)]
struct HeldKey {
    id: KeyId,
    down_at_ms: u64,
}

/// 纯逻辑判定器：输入原始键事件 + 时间戳，输出语义事件。无 IO，可单测。
pub struct HotkeyDetector {
    config: HotkeyConfig,
    /// 当前物理按住、且属于任一功能 chord 的键。
    held: Vec<HeldKey>,
    /// 完整功能 chord 首次成立的时间；partial chord 不启动计时。
    gesture_started_at_ms: Option<u64>,
    /// 已让路：普通键介入后，剩余触发键释放不再产生事件
    yielded: bool,
    current_mode: Option<SessionMode>,
}

impl HotkeyDetector {
    pub fn new(config: HotkeyConfig) -> Self {
        Self {
            config: config.normalized(),
            held: Vec::new(),
            gesture_started_at_ms: None,
            yielded: false,
            current_mode: None,
        }
    }

    /// Ends any active gesture before replacing its chord definitions.
    pub fn set_config(&mut self, config: HotkeyConfig, t_ms: u64) -> Vec<HotkeyEvent> {
        let config = config.normalized();
        if self.config == config {
            return Vec::new();
        }
        if self.config.same_chords(&config) {
            self.config = config;
            return Vec::new();
        }
        let events = if self.current_mode.is_some() && !self.yielded {
            vec![HotkeyEvent::TriggerUp {
                held_ms: t_ms.saturating_sub(self.gesture_started_at_ms.unwrap_or(t_ms)),
            }]
        } else {
            Vec::new()
        };
        self.config = config;
        self.reset_gesture();
        events
    }

    /// 处理一个原始键事件。返回零或多个语义事件。
    pub fn on_key(&mut self, key: &str, down: bool, t_ms: u64) -> Vec<HotkeyEvent> {
        let key = canonical_key_id(key);
        if down {
            self.on_down(key.as_ref(), t_ms)
        } else {
            self.on_up(key.as_ref(), t_ms)
        }
    }

    fn chord_active(&self, chord: &[KeyId]) -> bool {
        !chord.is_empty()
            && chord
                .iter()
                .all(|key| self.held.iter().any(|held| held.id == *key))
    }

    fn completed_mode(&self) -> Option<SessionMode> {
        if self.chord_active(&self.config.translation) {
            Some(SessionMode::Translation)
        } else if self.chord_active(&self.config.dictation) {
            Some(SessionMode::Dictation)
        } else if self.chord_active(&self.config.assistant) {
            Some(SessionMode::Assistant)
        } else {
            None
        }
    }

    fn reset_gesture(&mut self) {
        self.held.clear();
        self.gesture_started_at_ms = None;
        self.yielded = false;
        self.current_mode = None;
    }

    #[cfg(target_os = "windows")]
    pub(super) fn has_active_gesture(&self) -> bool {
        self.current_mode.is_some() && !self.yielded
    }

    fn activate_completed_chord(&mut self, t_ms: u64) -> Vec<HotkeyEvent> {
        let Some(mode) = self.completed_mode() else {
            return Vec::new();
        };
        match self.current_mode {
            None => {
                self.current_mode = Some(mode);
                self.gesture_started_at_ms = Some(t_ms);
                vec![HotkeyEvent::TriggerDown { mode }]
            }
            Some(current)
                if mode == SessionMode::Translation && current != SessionMode::Translation =>
            {
                self.current_mode = Some(SessionMode::Translation);
                vec![HotkeyEvent::ModeUpgraded {
                    mode: SessionMode::Translation,
                }]
            }
            Some(_) => Vec::new(),
        }
    }

    fn on_down(&mut self, key: &str, t_ms: u64) -> Vec<HotkeyEvent> {
        if key == "Escape" {
            return Vec::new();
        }
        if self.config.is_trigger_key(key) {
            if let Some(existing) = self.held.iter().find(|held| held.id == key) {
                let stale = supports_stale_release_recovery(key)
                    && t_ms.saturating_sub(existing.down_at_ms) >= STALE_DUPLICATE_DOWN_MS;
                if !stale {
                    return Vec::new();
                }

                let should_cancel_stale_recording = self.current_mode.is_some() && !self.yielded;
                self.reset_gesture();
                self.held.push(HeldKey {
                    id: key.to_string(),
                    down_at_ms: t_ms,
                });
                let mut events = Vec::with_capacity(2);
                if should_cancel_stale_recording {
                    events.push(HotkeyEvent::Yielded);
                }
                events.extend(self.activate_completed_chord(t_ms));
                return events;
            }

            self.held.push(HeldKey {
                id: key.to_string(),
                down_at_ms: t_ms,
            });
            if self.yielded {
                return Vec::new();
            }
            return self.activate_completed_chord(t_ms);
        }

        // A normal key suppresses both an active gesture and any partial chord
        // until all tracked trigger keys are released.
        if !self.held.is_empty() && !self.yielded {
            let was_active = self.current_mode.take().is_some();
            self.yielded = true;
            if was_active {
                vec![HotkeyEvent::Yielded]
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    }

    fn on_up(&mut self, key: &str, t_ms: u64) -> Vec<HotkeyEvent> {
        let before = self.held.len();
        self.held.retain(|held| held.id != key);
        if self.held.len() == before {
            return Vec::new(); // 不是我们跟踪的键
        }
        if self.held.is_empty() {
            let was_yielded = std::mem::take(&mut self.yielded);
            let was_active = self.current_mode.take().is_some();
            let started_at = self.gesture_started_at_ms.take();
            if was_yielded {
                Vec::new() // 让路后静默复位
            } else if was_active {
                vec![HotkeyEvent::TriggerUp {
                    held_ms: t_ms.saturating_sub(started_at.unwrap_or(t_ms)),
                }]
            } else {
                Vec::new() // partial chord 从未激活
            }
        } else {
            Vec::new() // 一次手势中先松一个键：等全部 tracked 键松开
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> HotkeyConfig {
        HotkeyConfig {
            dictation: vec!["MetaRight".into()],
            assistant: vec!["AltRight".into()],
            translation: vec!["MetaRight".into(), "AltRight".into()],
            esc_cancels: true,
        }
    }

    fn det() -> HotkeyDetector {
        HotkeyDetector::new(cfg())
    }

    #[test]
    fn push_to_talk_basic() {
        let mut d = det();
        assert_eq!(
            d.on_key("MetaRight", true, 0),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        assert_eq!(
            d.on_key("MetaRight", false, 800),
            vec![HotkeyEvent::TriggerUp { held_ms: 800 }]
        );
    }

    #[test]
    fn short_press_reports_exact_held_ms() {
        let mut d = det();
        d.on_key("MetaRight", true, 100);
        assert_eq!(
            d.on_key("MetaRight", false, 449),
            vec![HotkeyEvent::TriggerUp { held_ms: 349 }]
        );
    }

    #[test]
    fn long_press_boundary_reports_exact_held_ms() {
        let mut d = det();
        d.on_key("MetaRight", true, 100);
        assert_eq!(
            d.on_key("MetaRight", false, 451),
            vec![HotkeyEvent::TriggerUp { held_ms: 351 }]
        );
    }

    #[test]
    fn combo_upgrades_to_translation() {
        let mut d = det();
        assert_eq!(
            d.on_key("MetaRight", true, 0),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        assert_eq!(
            d.on_key("AltGr", true, 120),
            vec![HotkeyEvent::ModeUpgraded {
                mode: SessionMode::Translation
            }]
        );
        // 先松一个不产生事件，全松才 Up
        assert_eq!(d.on_key("MetaRight", false, 900), vec![]);
        assert_eq!(
            d.on_key("AltGr", false, 950),
            vec![HotkeyEvent::TriggerUp { held_ms: 950 }]
        );
    }

    #[test]
    fn combo_order_does_not_matter() {
        let mut d = det();
        assert_eq!(
            d.on_key("AltGr", true, 0),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Assistant
            }]
        );
        assert_eq!(
            d.on_key("MetaRight", true, 50),
            vec![HotkeyEvent::ModeUpgraded {
                mode: SessionMode::Translation
            }]
        );
    }

    #[test]
    fn normal_key_yields_and_release_is_silent() {
        let mut d = det();
        d.on_key("MetaRight", true, 0);
        assert_eq!(d.on_key("KeyC", true, 100), vec![HotkeyEvent::Yielded]);
        // 让路后：后续触发键操作静默直至全部释放
        assert_eq!(d.on_key("KeyC", false, 150), vec![]);
        assert_eq!(d.on_key("MetaRight", false, 200), vec![]);
        // 复位后恢复正常
        assert_eq!(
            d.on_key("MetaRight", true, 300),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
    }

    #[test]
    fn yield_only_fires_once() {
        let mut d = det();
        d.on_key("MetaRight", true, 0);
        assert_eq!(d.on_key("KeyC", true, 10), vec![HotkeyEvent::Yielded]);
        assert_eq!(d.on_key("KeyV", true, 20), vec![]);
    }

    #[test]
    fn altgr_sequence_not_mistaken_for_translation() {
        // Windows AltGr = ControlLeft + AltGr 连发；ControlLeft 非触发键。
        let mut d = HotkeyDetector::new(HotkeyConfig {
            dictation: vec!["ControlRight".into()],
            assistant: vec!["AltRight".into()],
            translation: vec!["ControlRight".into(), "AltRight".into()],
            esc_cancels: true,
        });
        // ControlLeft down：无触发键按住 → 无事件
        assert_eq!(d.on_key("ControlLeft", true, 0), vec![]);
        // AltGr down → 助手乐观启动（正常）
        assert_eq!(
            d.on_key("AltGr", true, 1),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Assistant
            }]
        );
        // 用户接着打字母（AltGr+E 输特殊字符）→ 让路
        assert_eq!(d.on_key("KeyE", true, 60), vec![HotkeyEvent::Yielded]);
        assert_eq!(d.on_key("KeyE", false, 90), vec![]);
        assert_eq!(d.on_key("AltGr", false, 120), vec![]);
        assert_eq!(d.on_key("ControlLeft", false, 121), vec![]);
    }

    #[test]
    fn esc_is_ignored_by_chord_detection_and_does_not_yield() {
        let mut d = det();
        assert_eq!(d.on_key("Escape", true, 0), vec![]);
        d.on_key("MetaRight", true, 100);
        assert_eq!(d.on_key("Escape", true, 150), vec![]);
        assert_eq!(
            d.on_key("MetaRight", false, 500),
            vec![HotkeyEvent::TriggerUp { held_ms: 400 }]
        );
    }

    #[test]
    fn esc_setting_does_not_change_detector_semantics() {
        let mut config = cfg();
        config.esc_cancels = false;
        let mut d = HotkeyDetector::new(config);
        assert_eq!(d.on_key("Escape", true, 0), vec![]);
        d.on_key("MetaRight", true, 100);
        assert_eq!(d.on_key("Escape", true, 150), vec![]);
        assert_eq!(
            d.on_key("MetaRight", false, 500),
            vec![HotkeyEvent::TriggerUp { held_ms: 400 }]
        );
    }

    #[test]
    fn escape_latch_rejects_idle_disabled_and_duplicate_claims() {
        let latch = EscCancellationLatch::default();
        assert_eq!(latch.try_claim(), None);

        latch.arm_cancellable(3);
        latch.set_enabled(false);
        assert_eq!(latch.try_claim(), None);
        latch.set_enabled(true);
        assert_eq!(latch.try_claim(), Some(3));
        assert_eq!(latch.try_claim(), None);

        latch.arm_cancellable(3);
        assert_eq!(latch.try_claim(), None);
        latch.arm_cancellable(4);
        assert_eq!(latch.try_claim(), Some(4));
    }

    #[test]
    fn escape_filter_never_claims_a_repeat_from_an_unowned_sequence() {
        let latch = EscCancellationLatch::default();
        let mut filter = EscKeyFilter::default();

        assert_eq!(filter.on_key(true, true, &latch), EscKeyDecision::default());
        latch.arm_cancellable(5);
        assert_eq!(filter.on_key(true, true, &latch), EscKeyDecision::default());
        assert_eq!(
            filter.on_key(false, true, &latch),
            EscKeyDecision::default()
        );
        assert_eq!(
            filter.on_key(true, true, &latch),
            EscKeyDecision {
                event: Some(HotkeyEvent::EscPressed { session_id: 5 }),
                swallow: true,
            }
        );
        assert_eq!(
            filter.on_key(false, false, &latch),
            EscKeyDecision {
                event: None,
                swallow: true,
            }
        );
    }

    #[test]
    fn escape_claim_and_session_completion_have_one_winner() {
        let latch = EscCancellationLatch::default();
        latch.arm_cancellable(6);
        assert_eq!(latch.try_claim(), Some(6));
        assert!(!latch.transition_to_idle(Some(6)));

        latch.disarm();
        latch.arm_cancellable(7);
        assert!(latch.transition_to_idle(Some(7)));
        assert_eq!(latch.try_claim(), None);
    }

    #[test]
    fn os_key_repeat_ignored() {
        let mut d = det();
        d.on_key("MetaRight", true, 0);
        assert_eq!(d.on_key("MetaRight", true, 30), vec![]);
        assert_eq!(d.on_key("MetaRight", true, 60), vec![]);
        assert_eq!(
            d.on_key("MetaRight", false, 500),
            vec![HotkeyEvent::TriggerUp { held_ms: 500 }]
        );
    }

    #[test]
    fn stale_duplicate_trigger_down_recovers_after_missed_release() {
        let mut d = det();
        assert_eq!(
            d.on_key("MetaRight", true, 0),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        assert_eq!(
            d.on_key("MetaRight", true, 500),
            vec![
                HotkeyEvent::Yielded,
                HotkeyEvent::TriggerDown {
                    mode: SessionMode::Dictation
                }
            ]
        );
        assert_eq!(
            d.on_key("MetaRight", false, 900),
            vec![HotkeyEvent::TriggerUp { held_ms: 400 }]
        );
    }

    #[test]
    fn stale_duplicate_after_yield_starts_fresh_session() {
        let mut d = det();
        d.on_key("MetaRight", true, 0);
        assert_eq!(d.on_key("KeyC", true, 40), vec![HotkeyEvent::Yielded]);
        assert_eq!(
            d.on_key("MetaRight", true, 500),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        assert_eq!(
            d.on_key("MetaRight", false, 700),
            vec![HotkeyEvent::TriggerUp { held_ms: 200 }]
        );
    }

    #[test]
    fn multi_key_chord_requires_every_member_and_partial_release_is_silent() {
        let mut d = HotkeyDetector::new(HotkeyConfig {
            dictation: vec!["ControlRight".into(), "Digit1".into()],
            assistant: vec!["AltRight".into(), "KeyA".into()],
            translation: Vec::new(),
            esc_cancels: true,
        });

        assert_eq!(d.on_key("ControlRight", true, 0), vec![]);
        assert_eq!(d.on_key("ControlRight", false, 10), vec![]);

        assert_eq!(d.on_key("ControlRight", true, 20), vec![]);
        assert_eq!(
            d.on_key("Digit1", true, 30),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        assert_eq!(d.on_key("Digit1", false, 70), vec![]);
        assert_eq!(
            d.on_key("ControlRight", false, 130),
            vec![HotkeyEvent::TriggerUp { held_ms: 100 }]
        );
    }

    #[test]
    fn custom_multi_key_chords_upgrade_when_translation_is_complete() {
        let mut d = HotkeyDetector::new(HotkeyConfig {
            dictation: vec!["ControlRight".into(), "Digit1".into()],
            assistant: vec!["AltRight".into(), "KeyA".into()],
            translation: vec![
                "ControlRight".into(),
                "Digit1".into(),
                "AltRight".into(),
                "KeyA".into(),
            ],
            esc_cancels: true,
        });

        assert_eq!(d.on_key("ControlRight", true, 0), vec![]);
        assert_eq!(
            d.on_key("Num1", true, 10),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        assert_eq!(d.on_key("AltGr", true, 20), vec![]);
        assert_eq!(
            d.on_key("KeyA", true, 30),
            vec![HotkeyEvent::ModeUpgraded {
                mode: SessionMode::Translation
            }]
        );

        for (key, at) in [("Digit1", 60), ("ControlRight", 70), ("KeyA", 80)] {
            assert_eq!(d.on_key(key, false, at), vec![]);
        }
        assert_eq!(
            d.on_key("AltRight", false, 110),
            vec![HotkeyEvent::TriggerUp { held_ms: 100 }]
        );
    }

    #[test]
    fn independent_translation_chord_triggers_directly() {
        let mut d = HotkeyDetector::new(HotkeyConfig {
            dictation: vec!["ControlRight".into()],
            assistant: vec!["AltRight".into()],
            translation: vec!["F13".into(), "Menu".into()],
            esc_cancels: true,
        });

        assert_eq!(d.on_key("F13", true, 0), vec![]);
        assert_eq!(
            d.on_key("Menu", true, 25),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Translation
            }]
        );
        assert_eq!(d.on_key("F13", false, 100), vec![]);
        assert_eq!(
            d.on_key("Menu", false, 225),
            vec![HotkeyEvent::TriggerUp { held_ms: 200 }]
        );
    }

    #[test]
    fn ordinary_key_silently_suppresses_an_unactivated_partial_chord() {
        let mut d = HotkeyDetector::new(HotkeyConfig {
            dictation: vec!["ControlRight".into(), "Digit1".into()],
            assistant: vec!["AltRight".into()],
            translation: Vec::new(),
            esc_cancels: true,
        });

        assert_eq!(d.on_key("ControlRight", true, 0), vec![]);
        assert_eq!(d.on_key("KeyC", true, 20), vec![]);
        assert_eq!(d.on_key("Digit1", true, 30), vec![]);
        assert_eq!(d.on_key("Digit1", false, 40), vec![]);
        assert_eq!(d.on_key("ControlRight", false, 50), vec![]);

        assert_eq!(d.on_key("ControlRight", true, 100), vec![]);
        assert_eq!(
            d.on_key("Digit1", true, 110),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
    }

    #[test]
    fn ordinary_trigger_auto_repeat_is_not_stale_release_recovery() {
        let mut d = HotkeyDetector::new(HotkeyConfig {
            dictation: vec!["KeyA".into()],
            assistant: vec!["F13".into()],
            translation: Vec::new(),
            esc_cancels: true,
        });

        assert_eq!(
            d.on_key("KeyA", true, 0),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        assert_eq!(d.on_key("KeyA", true, 500), vec![]);
        assert_eq!(
            d.on_key("KeyA", false, 700),
            vec![HotkeyEvent::TriggerUp { held_ms: 700 }]
        );
    }

    #[test]
    fn config_update_ends_active_single_chord_and_old_release_is_silent() {
        let mut d = det();
        assert_eq!(
            d.on_key("MetaRight", true, 100),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );

        assert_eq!(
            d.set_config(
                HotkeyConfig {
                    dictation: vec!["F13".into()],
                    assistant: vec!["F14".into()],
                    translation: Vec::new(),
                    esc_cancels: true,
                },
                449,
            ),
            vec![HotkeyEvent::TriggerUp { held_ms: 349 }]
        );
        assert_eq!(d.on_key("MetaRight", false, 500), vec![]);
    }

    #[test]
    fn identical_config_update_preserves_active_gesture() {
        let mut d = det();
        assert_eq!(
            d.on_key("MetaRight", true, 100),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );

        assert_eq!(d.set_config(cfg(), 200), vec![]);
        assert_eq!(
            d.on_key("MetaRight", false, 449),
            vec![HotkeyEvent::TriggerUp { held_ms: 349 }]
        );
    }

    #[test]
    fn esc_setting_update_preserves_active_gesture() {
        let mut d = det();
        d.on_key("MetaRight", true, 100);
        let mut replacement = cfg();
        replacement.esc_cancels = false;

        assert!(d.set_config(replacement, 200).is_empty());
        assert_eq!(
            d.on_key("MetaRight", false, 449),
            vec![HotkeyEvent::TriggerUp { held_ms: 349 }]
        );
    }

    #[test]
    fn pause_reset_discards_held_state_before_resume() {
        let mut d = det();
        assert_eq!(
            d.on_key("MetaRight", true, 0),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        d.reset_gesture();
        assert_eq!(d.on_key("MetaRight", false, 100), vec![]);
        assert_eq!(
            d.on_key("MetaRight", true, 110),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
    }

    #[test]
    fn config_update_ends_active_multi_chord_once_but_not_partial_chord() {
        let old = HotkeyConfig {
            dictation: vec!["ControlRight".into(), "Digit1".into()],
            assistant: vec!["AltRight".into(), "KeyA".into()],
            translation: Vec::new(),
            esc_cancels: true,
        };
        let replacement = HotkeyConfig {
            dictation: vec!["F13".into()],
            assistant: vec!["F14".into()],
            translation: Vec::new(),
            esc_cancels: true,
        };

        let mut partial = HotkeyDetector::new(old.clone());
        assert_eq!(partial.on_key("ControlRight", true, 0), vec![]);
        assert_eq!(partial.set_config(replacement.clone(), 100), vec![]);
        assert_eq!(partial.on_key("ControlRight", false, 110), vec![]);

        let mut active = HotkeyDetector::new(old);
        assert_eq!(active.on_key("ControlRight", true, 0), vec![]);
        assert_eq!(
            active.on_key("Digit1", true, 10),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        assert_eq!(
            active.set_config(replacement, 361),
            vec![HotkeyEvent::TriggerUp { held_ms: 351 }]
        );
        assert_eq!(active.on_key("Digit1", false, 400), vec![]);
        assert_eq!(active.on_key("ControlRight", false, 410), vec![]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn startup_failure_is_exposed_through_the_shared_health_receiver() {
        use windows_backend::{WindowsHookError, WindowsHookHealth};

        let error = WindowsHookError::Install { code: 5 };
        let runtime = ManagedWindowsHotkey::failed(error.clone());
        let health = runtime.subscribe_health();

        assert_eq!(runtime.health(), WindowsHookHealth::Failed(error.clone()));
        assert_eq!(*health.borrow(), WindowsHookHealth::Failed(error));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn unexpected_terminal_health_cancels_once_when_not_paused() {
        use windows_backend::{WindowsHookError, WindowsHookHealth};

        let mut latch = WindowsHookFailureLatch::default();
        let first = latch.observe(
            &WindowsHookHealth::Failed(WindowsHookError::MessageLoop { code: 5 }),
            false,
        );
        assert_eq!(
            first,
            WindowsHookHealthAction {
                cancel_session: true,
                refresh_status: true,
            }
        );
        assert_eq!(
            latch.observe(&WindowsHookHealth::Stopped, false),
            WindowsHookHealthAction::default()
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn healthy_paused_and_expected_shutdown_states_do_not_mis_cancel() {
        use windows_backend::{WindowsHookError, WindowsHookHealth};

        let mut normal = WindowsHookFailureLatch::default();
        assert_eq!(
            normal.observe(&WindowsHookHealth::Starting, false),
            WindowsHookHealthAction::default()
        );
        assert_eq!(
            normal.observe(&WindowsHookHealth::Healthy, false),
            WindowsHookHealthAction::default()
        );
        assert_eq!(
            normal.observe(&WindowsHookHealth::Shutdown, false),
            WindowsHookHealthAction::default()
        );

        let mut paused = WindowsHookFailureLatch::default();
        assert_eq!(
            paused.observe(
                &WindowsHookHealth::Failed(WindowsHookError::CallbackPanicked),
                true,
            ),
            WindowsHookHealthAction {
                cancel_session: false,
                refresh_status: true,
            }
        );
    }
}
