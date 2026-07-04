//! 全部配置结构体 + 默认值 + schema_version（07 §4 settings/schema.rs）。
//! settings.json 形态见 03 §6；本文件是其唯一 Rust 定义。

use crate::types::profile::{ProviderProfile, SlotKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub general: GeneralSettings,
    pub dictation: DictationSettings,
    pub translation: TranslationSettings,
    pub assistant: AssistantSettings,
    pub hotkeys: HotkeySettings,
    pub history: HistorySettings,
    /// 槽位 → active profile id
    pub slots: HashMap<SlotKind, SlotConfig>,
    pub profiles: Vec<ProviderProfile>,
    pub onboarding_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            general: GeneralSettings::default(),
            dictation: DictationSettings::default(),
            translation: TranslationSettings::default(),
            assistant: AssistantSettings::default(),
            hotkeys: HotkeySettings::default(),
            history: HistorySettings::default(),
            slots: SlotKind::ALL.iter().map(|k| (*k, SlotConfig::default())).collect(),
            profiles: Vec::new(),
            onboarding_done: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct SlotConfig {
    pub active_profile: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UiLanguage {
    #[default]
    System,
    ZhCn,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    #[default]
    System,
    Manual,
    Direct,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct GeneralSettings {
    pub language: UiLanguage,
    pub theme: ThemeMode,
    pub autostart: bool,
    pub chimes_enabled: bool,
    /// 0.0–1.0
    pub chimes_volume: f32,
    pub proxy_mode: ProxyMode,
    /// proxy_mode = Manual 时生效，如 "socks5://127.0.0.1:1080"
    pub proxy_url: String,
    pub check_updates: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            language: UiLanguage::System,
            theme: ThemeMode::System,
            autostart: true,
            chimes_enabled: true,
            chimes_volume: 0.6,
            proxy_mode: ProxyMode::System,
            proxy_url: String::new(),
            check_updates: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum InjectMethod {
    #[default]
    Auto,
    Paste,
    TypeDirect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct DictationSettings {
    /// F-9 整理层开关（默认开，关闭 = 原样模式）
    pub polish_enabled: bool,
    /// 自定义整理提示词；空 = 用内置模板（03 §3.4）
    pub polish_prompt: String,
    pub inject_method: InjectMethod,
    /// 粘贴前延迟 ms（平台坑 7.2-4）
    pub paste_delay_ms: u64,
    /// STT language 提示；"auto" = 自动检测
    pub language: String,
    /// 固定麦克风设备名；空 = 系统默认
    pub microphone: String,
    /// Esc 取消录音（05 §3.3 可关）
    pub esc_cancels: bool,
}

impl Default for DictationSettings {
    fn default() -> Self {
        Self {
            polish_enabled: true,
            polish_prompt: String::new(),
            inject_method: InjectMethod::Auto,
            paste_delay_ms: 60,
            language: "auto".into(),
            microphone: String::new(),
            esc_cancels: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct TranslationSettings {
    /// 源语言（你说话使用的语言）
    pub source_language: String,
    /// 目标语言
    pub target_language: String,
    /// 双向翻译（默认开，02 F-2）
    pub bidirectional: bool,
    /// 自定义翻译提示词；空 = 内置模板
    pub translate_prompt: String,
    /// 最近使用过的目标语言（HUD 快切）
    pub recent_targets: Vec<String>,
}

impl Default for TranslationSettings {
    fn default() -> Self {
        Self {
            source_language: "中文（简体）".into(),
            target_language: "English".into(),
            bidirectional: true,
            translate_prompt: String::new(),
            recent_targets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RewriteDisposition {
    /// 改写型结果自动替换选区（默认）
    #[default]
    AutoReplace,
    /// 面板中预览，Enter 替换 / Esc 放弃
    Preview,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct AssistantSettings {
    pub disposition: RewriteDisposition,
    /// 自定义处理/问答提示词；空 = 内置模板
    pub process_prompt: String,
    pub ask_prompt: String,
}

/// 快捷键绑定：一组按键标识（rdev key 的稳定字符串名）。
/// 默认全修饰键三角方案（ADR-7）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct HotkeySettings {
    pub dictation: Vec<String>,
    pub assistant: Vec<String>,
    pub translation: Vec<String>,
    /// 长按/短按判定阈值 ms（02 F-5，可调）
    pub hold_threshold_ms: u64,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        // 注意：rdev 中右 ⌥/右 Alt 的标识是 "AltGr"（rdev::Key 无 AltRight 变体）
        #[cfg(target_os = "macos")]
        let (dict, assist) = (vec!["MetaRight".to_string()], vec!["AltGr".to_string()]);
        #[cfg(not(target_os = "macos"))]
        let (dict, assist) = (vec!["ControlRight".to_string()], vec!["AltGr".to_string()]);
        let translation = vec![dict[0].clone(), assist[0].clone()];
        Self { dictation: dict, assistant: assist, translation, hold_threshold_ms: 350 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct HistorySettings {
    pub enabled: bool,
    /// 保留天数；0 = 永久
    pub retention_days: u32,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self { enabled: true, retention_days: 90 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_roundtrip() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn unknown_fields_do_not_break_parsing() {
        let json = r#"{ "schema_version": 1, "future_field": {"x": 1} }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.schema_version, 1);
    }

    #[test]
    fn default_hotkeys_are_triangle_scheme() {
        let h = HotkeySettings::default();
        assert_eq!(h.hold_threshold_ms, 350);
        assert_eq!(h.translation.len(), 2);
        assert!(h.translation.contains(&h.dictation[0]));
        assert!(h.translation.contains(&h.assistant[0]));
    }
}
