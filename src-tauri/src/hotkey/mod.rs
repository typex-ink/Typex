//! 全局快捷键：trait HotkeyBackend + 纯逻辑判定器（07 §7.3）。
//!
//! rdev 线程只做「原始键事件 → Detector 判定 → mpsc 发送语义事件」；
//! 长按/短按与会话语义由 orchestrator 状态机处理，本层只负责：
//! - 触发键识别（含右⌘+右⌥ 组合升级为翻译）
//! - 组合键让路（触发键按住期间出现普通键 → Yield）
//! - Esc 透传

pub mod rdev_backend;

use crate::types::SessionMode;

/// 键的稳定字符串标识（rdev::Key 的 Debug 名，settings.json 存储形态）。
pub type KeyId = String;

/// 判定器输出的语义事件（发往 orchestrator）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// 触发键按下（乐观启动录音）
    TriggerDown { mode: SessionMode },
    /// 按住期间组合出另一触发键 → 升级为翻译（音频保留）
    ModeUpgraded { mode: SessionMode },
    /// 全部触发键松开；held_ms 自首个触发键按下起算
    TriggerUp { held_ms: u64 },
    /// 组合键让路：触发键按住期间出现普通键 → 静默取消
    Yielded,
    /// Esc 按下（listen-only 不吞键；仅 Recording 态有效由状态机决定）
    EscPressed,
}

/// 判定器配置：各功能的触发键组。
#[derive(Debug, Clone)]
pub struct HotkeyConfig {
    pub dictation: Vec<KeyId>,
    pub assistant: Vec<KeyId>,
    /// 翻译 = 两触发键同按（默认 dictation + assistant 各一键）
    pub translation: Vec<KeyId>,
}

impl HotkeyConfig {
    pub fn from_settings(h: &crate::settings::schema::HotkeySettings) -> Self {
        Self {
            dictation: h.dictation.clone(),
            assistant: h.assistant.clone(),
            translation: h.translation.clone(),
        }
    }

    fn role_of(&self, key: &str) -> Option<SessionMode> {
        if self.dictation.iter().any(|k| k == key) {
            Some(SessionMode::Dictation)
        } else if self.assistant.iter().any(|k| k == key) {
            Some(SessionMode::Assistant)
        } else {
            None
        }
    }
}

/// 纯逻辑判定器：输入原始键事件 + 时间戳，输出语义事件。无 IO，可单测。
pub struct HotkeyDetector {
    config: HotkeyConfig,
    /// 当前按住的触发键（KeyId → 角色）
    held: Vec<(KeyId, SessionMode)>,
    first_down_at: u64,
    /// 已让路：普通键介入后，剩余触发键释放不再产生事件
    yielded: bool,
    current_mode: Option<SessionMode>,
}

impl HotkeyDetector {
    pub fn new(config: HotkeyConfig) -> Self {
        Self {
            config,
            held: Vec::new(),
            first_down_at: 0,
            yielded: false,
            current_mode: None,
        }
    }

    pub fn set_config(&mut self, config: HotkeyConfig) {
        self.config = config;
        self.held.clear();
        self.yielded = false;
        self.current_mode = None;
    }

    /// 处理一个原始键事件。返回零或多个语义事件。
    pub fn on_key(&mut self, key: &str, down: bool, t_ms: u64) -> Vec<HotkeyEvent> {
        if down {
            self.on_down(key, t_ms)
        } else {
            self.on_up(key, t_ms)
        }
    }

    fn translation_active(&self) -> bool {
        // 翻译组合成立 = 组合中每个键都在按住集合里
        !self.config.translation.is_empty()
            && self
                .config
                .translation
                .iter()
                .all(|k| self.held.iter().any(|(h, _)| h == k))
    }

    fn on_down(&mut self, key: &str, t_ms: u64) -> Vec<HotkeyEvent> {
        if key == "Escape" && self.held.is_empty() {
            return vec![HotkeyEvent::EscPressed];
        }
        match self.config.role_of(key) {
            Some(role) => {
                if self.held.iter().any(|(h, _)| h == key) {
                    return vec![]; // OS 自动重复
                }
                self.held.push((key.to_string(), role));
                if self.yielded {
                    return vec![];
                }
                if self.held.len() == 1 {
                    self.first_down_at = t_ms;
                    self.current_mode = Some(role);
                    vec![HotkeyEvent::TriggerDown { mode: role }]
                } else if self.translation_active()
                    && self.current_mode != Some(SessionMode::Translation)
                {
                    self.current_mode = Some(SessionMode::Translation);
                    vec![HotkeyEvent::ModeUpgraded {
                        mode: SessionMode::Translation,
                    }]
                } else {
                    vec![]
                }
            }
            None => {
                // 组合键让路：触发键按住期间出现任何普通键
                if !self.held.is_empty() && !self.yielded {
                    self.yielded = true;
                    self.current_mode = None;
                    vec![HotkeyEvent::Yielded]
                } else {
                    vec![]
                }
            }
        }
    }

    fn on_up(&mut self, key: &str, t_ms: u64) -> Vec<HotkeyEvent> {
        let before = self.held.len();
        self.held.retain(|(h, _)| h != key);
        if self.held.len() == before {
            return vec![]; // 不是我们跟踪的键
        }
        if self.held.is_empty() {
            let was_yielded = std::mem::take(&mut self.yielded);
            self.current_mode = None;
            if was_yielded {
                vec![] // 让路后静默复位
            } else {
                vec![HotkeyEvent::TriggerUp {
                    held_ms: t_ms.saturating_sub(self.first_down_at),
                }]
            }
        } else {
            vec![] // 组合中先松一个键：等全部松开
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> HotkeyConfig {
        HotkeyConfig {
            dictation: vec!["MetaRight".into()],
            assistant: vec!["AltGr".into()],
            translation: vec!["MetaRight".into(), "AltGr".into()],
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
            assistant: vec!["AltGr".into()],
            translation: vec!["ControlRight".into(), "AltGr".into()],
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
    fn esc_passthrough_only_when_no_trigger_held() {
        let mut d = det();
        assert_eq!(d.on_key("Escape", true, 0), vec![HotkeyEvent::EscPressed]);
        // 触发键按住期间 Esc 是普通键 → 让路
        d.on_key("MetaRight", true, 100);
        assert_eq!(d.on_key("Escape", true, 150), vec![HotkeyEvent::Yielded]);
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
}
