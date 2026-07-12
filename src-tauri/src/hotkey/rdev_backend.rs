//! rdev 独立线程监听（macOS/Windows/X11，06 §7.3）。
//! listen-only：不拦截任何按键；判定在 HotkeyDetector 纯逻辑层。

use super::{HotkeyConfig, HotkeyDetector, HotkeyEvent};
use std::sync::mpsc as std_mpsc;
use std::time::Instant;
use tokio::sync::{mpsc, watch};

/// 启动 rdev 监听线程。语义事件经返回的 receiver 消费。
/// `config_rx`：设置变更时热更新键位。
/// `paused_rx`：托盘「暂停 Typex」时置 true，事件全部丢弃。
pub fn spawn(
    initial: HotkeyConfig,
    mut config_rx: watch::Receiver<HotkeyConfig>,
    mut paused_rx: watch::Receiver<bool>,
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
            let mut detector = HotkeyDetector::new(initial);
            let epoch = Instant::now();
            let mut paused = *paused_rx.borrow_and_update();
            let result = rdev::listen(move |event: rdev::Event| {
                let t_ms = epoch.elapsed().as_millis() as u64;
                refresh_pause_state(&mut detector, &mut paused_rx, &mut paused);
                // 应用热更新（非阻塞轮询）
                while let Ok(cfg) = cfg_rx_std.try_recv() {
                    for semantic in detector.set_config(cfg, t_ms) {
                        let _ = tx.send(semantic);
                    }
                }
                if paused {
                    return;
                }
                let (key, down) = match event.event_type {
                    rdev::EventType::KeyPress(k) => (k, true),
                    rdev::EventType::KeyRelease(k) => (k, false),
                    _ => return,
                };
                for ev in detector.on_key(&key_id(key), down, t_ms) {
                    let _ = tx.send(ev);
                }
            });
            if let Err(e) = result {
                // macOS 未授权辅助功能时 rdev 静默无事件或直接失败（平台坑 7.2-1）
                tracing::error!("rdev listen 失败（缺辅助功能权限？）: {e:?}");
            }
        })
        .expect("spawn hotkey thread");

    rx
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
    use super::{HotkeyConfig, HotkeyDetector, HotkeyEvent, refresh_pause_state};
    use crate::types::SessionMode;

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
}
