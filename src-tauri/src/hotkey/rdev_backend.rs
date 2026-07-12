//! rdev 独立线程监听（macOS grab / X11 listen-only，06 §7.3）。

use super::{EscCancellationLatch, EscKeyFilter, HotkeyConfig, HotkeyDetector, HotkeyEvent};
use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use std::time::Instant;
use tokio::sync::{mpsc, watch};

/// 启动 rdev 监听线程。语义事件经返回的 receiver 消费。
/// `config_rx`：设置变更时热更新键位。
/// `paused_rx`：托盘「暂停 Typex」时置 true，事件全部丢弃。
pub fn spawn(
    initial: HotkeyConfig,
    mut config_rx: watch::Receiver<HotkeyConfig>,
    mut paused_rx: watch::Receiver<bool>,
    escape_latch: Arc<EscCancellationLatch>,
) -> mpsc::UnboundedReceiver<HotkeyEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    // rdev::listen 是阻塞调用且要求整线程；配置热更新经 std mpsc 转发进回调。
    let (cfg_tx, cfg_rx_std) = std_mpsc::channel::<HotkeyConfig>();
    std::thread::Builder::new()
        .name("typex-hotkey-cfg".into())
        .spawn(move || {
            // watch → std channel 桥接线程（轮询：配置变更不频繁，500ms 足够）
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                match config_rx.has_changed() {
                    Ok(true) => {
                        let cfg = config_rx.borrow_and_update().clone();
                        if cfg_tx.send(cfg).is_err() {
                            break;
                        }
                    }
                    Ok(false) => {}
                    Err(_) => break, // sender 已 drop
                }
            }
        })
        .expect("spawn hotkey cfg thread");

    std::thread::Builder::new()
        .name("typex-hotkey".into())
        .spawn(move || {
            let paused = *paused_rx.borrow_and_update();
            let state = Mutex::new(RdevEventState {
                detector: HotkeyDetector::new(initial),
                config_rx: cfg_rx_std,
                paused,
                paused_rx,
                escape_latch,
                escape_filter: EscKeyFilter::default(),
                tx,
                epoch: Instant::now(),
            });
            #[cfg(target_os = "macos")]
            let result = rdev::grab(move |event: rdev::Event| {
                if state.lock().unwrap().process(&event) {
                    None
                } else {
                    Some(event)
                }
            });
            #[cfg(not(target_os = "macos"))]
            let result = rdev::listen(move |event: rdev::Event| {
                let _ = state.lock().unwrap().process(&event);
            });
            if let Err(e) = result {
                // macOS 未授权辅助功能时 rdev 静默无事件或直接失败（平台坑 7.2-1）
                tracing::error!("rdev hotkey backend 失败（缺辅助功能权限？）: {e:?}");
            }
        })
        .expect("spawn hotkey thread");

    rx
}

struct RdevEventState {
    detector: HotkeyDetector,
    config_rx: std_mpsc::Receiver<HotkeyConfig>,
    paused_rx: watch::Receiver<bool>,
    paused: bool,
    escape_latch: Arc<EscCancellationLatch>,
    escape_filter: EscKeyFilter,
    tx: mpsc::UnboundedSender<HotkeyEvent>,
    epoch: Instant,
}

impl RdevEventState {
    /// Returns true only for an Escape event owned by the current Typex session.
    fn process(&mut self, event: &rdev::Event) -> bool {
        let t_ms = self.epoch.elapsed().as_millis() as u64;
        refresh_pause_state(&mut self.detector, &mut self.paused_rx, &mut self.paused);
        while let Ok(config) = self.config_rx.try_recv() {
            for semantic in self.detector.set_config(config, t_ms) {
                let _ = self.tx.send(semantic);
            }
        }

        let (key, down) = match event.event_type {
            rdev::EventType::KeyPress(key) => (key, true),
            rdev::EventType::KeyRelease(key) => (key, false),
            _ => return false,
        };
        let key = key_id(key);
        if key == "Escape" {
            let decision = self
                .escape_filter
                .on_key(down, !self.paused, &self.escape_latch);
            if let Some(event) = decision.event {
                let _ = self.tx.send(event);
            }
            return decision.swallow;
        }
        if self.paused {
            return false;
        }
        for event in self.detector.on_key(&key, down, t_ms) {
            let _ = self.tx.send(event);
        }
        false
    }
}

fn refresh_pause_state(
    detector: &mut HotkeyDetector,
    paused_rx: &mut watch::Receiver<bool>,
    paused: &mut bool,
) {
    if matches!(paused_rx.has_changed(), Ok(true)) {
        *paused = *paused_rx.borrow_and_update();
        detector.reset_gesture();
    }
}

/// rdev physical key variant → stable persisted KeyId.
pub fn key_id(key: rdev::Key) -> String {
    use rdev::Key;

    let id = match key {
        Key::Alt => "AltLeft",
        Key::AltGr => "AltRight",
        Key::Backspace => "Backspace",
        Key::CapsLock => "CapsLock",
        Key::ControlLeft => "ControlLeft",
        Key::ControlRight => "ControlRight",
        Key::Delete => "Delete",
        Key::DownArrow => "ArrowDown",
        Key::End => "End",
        Key::Escape => "Escape",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::Home => "Home",
        Key::LeftArrow => "ArrowLeft",
        Key::MetaLeft => "MetaLeft",
        Key::MetaRight => "MetaRight",
        Key::PageDown => "PageDown",
        Key::PageUp => "PageUp",
        Key::Return => "Enter",
        Key::RightArrow => "ArrowRight",
        Key::ShiftLeft => "ShiftLeft",
        Key::ShiftRight => "ShiftRight",
        Key::Space => "Space",
        Key::Tab => "Tab",
        Key::UpArrow => "ArrowUp",
        Key::PrintScreen => "PrintScreen",
        Key::ScrollLock => "ScrollLock",
        Key::Pause => "Pause",
        Key::NumLock => "NumLock",
        Key::BackQuote => "Backquote",
        Key::Num1 => "Digit1",
        Key::Num2 => "Digit2",
        Key::Num3 => "Digit3",
        Key::Num4 => "Digit4",
        Key::Num5 => "Digit5",
        Key::Num6 => "Digit6",
        Key::Num7 => "Digit7",
        Key::Num8 => "Digit8",
        Key::Num9 => "Digit9",
        Key::Num0 => "Digit0",
        Key::Minus => "Minus",
        Key::Equal => "Equal",
        Key::KeyQ => "KeyQ",
        Key::KeyW => "KeyW",
        Key::KeyE => "KeyE",
        Key::KeyR => "KeyR",
        Key::KeyT => "KeyT",
        Key::KeyY => "KeyY",
        Key::KeyU => "KeyU",
        Key::KeyI => "KeyI",
        Key::KeyO => "KeyO",
        Key::KeyP => "KeyP",
        Key::LeftBracket => "BracketLeft",
        Key::RightBracket => "BracketRight",
        Key::KeyA => "KeyA",
        Key::KeyS => "KeyS",
        Key::KeyD => "KeyD",
        Key::KeyF => "KeyF",
        Key::KeyG => "KeyG",
        Key::KeyH => "KeyH",
        Key::KeyJ => "KeyJ",
        Key::KeyK => "KeyK",
        Key::KeyL => "KeyL",
        Key::SemiColon => "Semicolon",
        Key::Quote => "Quote",
        Key::BackSlash => "Backslash",
        Key::IntlBackslash => "IntlBackslash",
        Key::KeyZ => "KeyZ",
        Key::KeyX => "KeyX",
        Key::KeyC => "KeyC",
        Key::KeyV => "KeyV",
        Key::KeyB => "KeyB",
        Key::KeyN => "KeyN",
        Key::KeyM => "KeyM",
        Key::Comma => "Comma",
        Key::Dot => "Period",
        Key::Slash => "Slash",
        Key::Insert => "Insert",
        Key::KpReturn => "NumpadEnter",
        Key::KpMinus => "NumpadSubtract",
        Key::KpPlus => "NumpadAdd",
        Key::KpMultiply => "NumpadMultiply",
        Key::KpDivide => "NumpadDivide",
        Key::Kp0 => "Numpad0",
        Key::Kp1 => "Numpad1",
        Key::Kp2 => "Numpad2",
        Key::Kp3 => "Numpad3",
        Key::Kp4 => "Numpad4",
        Key::Kp5 => "Numpad5",
        Key::Kp6 => "Numpad6",
        Key::Kp7 => "Numpad7",
        Key::Kp8 => "Numpad8",
        Key::Kp9 => "Numpad9",
        Key::KpDelete => "NumpadDecimal",
        Key::Function => "Fn",
        Key::Unknown(code) => return unknown_key_id(code),
    };
    id.to_string()
}

fn unknown_key_id(code: u32) -> String {
    #[cfg(target_os = "macos")]
    let known = match code {
        105 => Some("F13"),
        107 => Some("F14"),
        113 => Some("F15"),
        106 => Some("F16"),
        64 => Some("F17"),
        79 => Some("F18"),
        80 => Some("F19"),
        _ => None,
    };
    #[cfg(target_os = "linux")]
    let known = match code {
        135 => Some("Menu"),
        191 => Some("F13"),
        192 => Some("F14"),
        193 => Some("F15"),
        194 => Some("F16"),
        195 => Some("F17"),
        196 => Some("F18"),
        197 => Some("F19"),
        _ => None,
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let known: Option<&str> = None;

    known
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Unknown({code})"))
}

#[cfg(test)]
mod tests {
    use super::{
        EscCancellationLatch, HotkeyConfig, HotkeyDetector, HotkeyEvent, RdevEventState,
        refresh_pause_state,
    };
    use crate::types::SessionMode;
    use std::sync::{Arc, mpsc as std_mpsc};
    use std::time::{Instant, SystemTime};
    use tokio::sync::{mpsc, watch};

    fn event(event_type: rdev::EventType) -> rdev::Event {
        rdev::Event {
            time: SystemTime::now(),
            name: None,
            event_type,
        }
    }

    fn event_state(
        latch: Arc<EscCancellationLatch>,
    ) -> (RdevEventState, mpsc::UnboundedReceiver<HotkeyEvent>) {
        let (_config_tx, config_rx) = std_mpsc::channel();
        let (_paused_tx, paused_rx) = watch::channel(false);
        let (tx, rx) = mpsc::unbounded_channel();
        (
            RdevEventState {
                detector: HotkeyDetector::new(HotkeyConfig {
                    dictation: vec!["MetaRight".into()],
                    assistant: vec!["AltRight".into()],
                    translation: vec!["MetaRight".into(), "AltRight".into()],
                    esc_cancels: true,
                }),
                config_rx,
                paused_rx,
                paused: false,
                escape_latch: latch,
                escape_filter: Default::default(),
                tx,
                epoch: Instant::now(),
            },
            rx,
        )
    }

    #[test]
    fn key_id_stable_names() {
        assert_eq!(super::key_id(rdev::Key::MetaRight), "MetaRight");
        assert_eq!(super::key_id(rdev::Key::AltGr), "AltRight");
        assert_eq!(super::key_id(rdev::Key::ControlRight), "ControlRight");
        assert_eq!(super::key_id(rdev::Key::Escape), "Escape");
        assert_eq!(super::key_id(rdev::Key::Return), "Enter");
        assert_eq!(super::key_id(rdev::Key::Num1), "Digit1");
        assert_eq!(super::key_id(rdev::Key::LeftArrow), "ArrowLeft");
        assert_eq!(super::key_id(rdev::Key::SemiColon), "Semicolon");
    }

    #[test]
    fn pause_version_change_resets_held_keys_even_when_values_coalesce() {
        let config = HotkeyConfig {
            dictation: vec!["MetaRight".into()],
            assistant: vec!["AltRight".into()],
            translation: Vec::new(),
            esc_cancels: true,
        };
        let mut detector = HotkeyDetector::new(config);
        let (paused_tx, mut paused_rx) = tokio::sync::watch::channel(false);
        let mut paused = *paused_rx.borrow_and_update();

        assert_eq!(
            detector.on_key("MetaRight", true, 0),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
        paused_tx.send_replace(true);
        paused_tx.send_replace(false);
        refresh_pause_state(&mut detector, &mut paused_rx, &mut paused);

        assert!(!paused);
        assert_eq!(detector.on_key("MetaRight", false, 100), vec![]);
        assert_eq!(
            detector.on_key("MetaRight", true, 110),
            vec![HotkeyEvent::TriggerDown {
                mode: SessionMode::Dictation
            }]
        );
    }

    #[test]
    fn adapter_consumes_only_an_owned_escape_sequence() {
        let latch = Arc::new(EscCancellationLatch::default());
        latch.arm_cancellable(7);
        let (mut state, mut rx) = event_state(latch.clone());

        assert!(state.process(&event(rdev::EventType::KeyPress(rdev::Key::Escape))));
        assert_eq!(
            rx.try_recv().unwrap(),
            HotkeyEvent::EscPressed { session_id: 7 }
        );
        assert!(state.process(&event(rdev::EventType::KeyPress(rdev::Key::Escape))));
        assert!(rx.try_recv().is_err());
        assert!(!state.process(&event(rdev::EventType::MouseMove { x: 1.0, y: 2.0 })));
        assert!(!state.process(&event(rdev::EventType::KeyPress(rdev::Key::KeyA))));
        assert!(state.process(&event(rdev::EventType::KeyRelease(rdev::Key::Escape))));

        latch.disarm();
        assert!(!state.process(&event(rdev::EventType::KeyPress(rdev::Key::Escape))));
        assert!(!state.process(&event(rdev::EventType::KeyRelease(rdev::Key::Escape))));
        assert!(rx.try_recv().is_err());
    }
}
