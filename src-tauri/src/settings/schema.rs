//! 全部配置结构体 + 默认值 + schema_version（07 §4 settings/schema.rs）。
//! settings.json 形态见 03 §6；本文件是其唯一 Rust 定义。

use crate::types::profile::{ModelDownloadSource, ProviderProfile, SlotKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const CURRENT_SCHEMA_VERSION: u32 = 3;

pub const DICTIONARY_MAX_TERMS: usize = 100;
pub const DICTIONARY_MAX_TERM_CHARS: usize = 50;
pub const DICTIONARY_MAX_TOTAL_CHARS: usize = 5_000;

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
    pub dictionary: DictionarySettings,
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
            dictionary: DictionarySettings::default(),
            slots: SlotKind::ALL
                .iter()
                .map(|k| (*k, SlotConfig::default()))
                .collect(),
            profiles: Vec::new(),
            onboarding_done: false,
        }
    }
}

impl Settings {
    pub fn normalize_for_save(&mut self) {
        self.schema_version = CURRENT_SCHEMA_VERSION;
        self.dictionary.normalize();
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
    /// 本地模型下载源（模型管理页底部设置）。
    pub model_download_source: ModelDownloadSource,
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
            model_download_source: ModelDownloadSource::Auto,
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

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct AssistantSettings {
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
        Self {
            dictation: dict,
            assistant: assist,
            translation,
            hold_threshold_ms: 350,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct HistorySettings {
    pub enabled: bool,
    /// 保留天数；0 = 永久
    pub retention_days: u32,
    /// 打字基准（字/分）——统计卡「节省时间」折算用（05 §8）
    pub typing_wpm: u32,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 90,
            typing_wpm: 45,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct DictionarySettings {
    /// 手动维护的高频词/专有名词表（F-10 v1：不区分来源/分类）。
    pub terms: Vec<String>,
}

impl DictionarySettings {
    pub fn normalize(&mut self) {
        self.terms = normalize_dictionary_terms(self.terms.iter().map(String::as_str));
    }

    pub fn normalized_terms(&self) -> Vec<String> {
        normalize_dictionary_terms(self.terms.iter().map(String::as_str))
    }

    pub fn stt_prompt(&self) -> Option<String> {
        let terms = self.normalized_terms();
        if terms.is_empty() {
            None
        } else {
            Some(terms.join("\n"))
        }
    }

    pub fn llm_context(&self) -> Option<String> {
        let terms = self.normalized_terms();
        if terms.is_empty() {
            return None;
        }
        Some(
            terms
                .into_iter()
                .map(|term| format!("- {}", escape_dictionary_xml(&term)))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

pub fn normalize_dictionary_terms<'a>(terms: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut total_chars = 0usize;

    for raw in terms {
        if out.len() >= DICTIONARY_MAX_TERMS {
            break;
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let term: String = trimmed.chars().take(DICTIONARY_MAX_TERM_CHARS).collect();
        if term.is_empty() || !seen.insert(term.clone()) {
            continue;
        }
        let chars = term.chars().count();
        if total_chars + chars > DICTIONARY_MAX_TOTAL_CHARS {
            break;
        }
        total_chars += chars;
        out.push(term);
    }

    out
}

fn escape_dictionary_xml(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for ch in term.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
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
        let json = r#"{ "schema_version": 3, "future_field": {"x": 1} }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.schema_version, 3);
    }

    #[test]
    fn default_hotkeys_are_triangle_scheme() {
        let h = HotkeySettings::default();
        assert_eq!(h.hold_threshold_ms, 350);
        assert_eq!(h.translation.len(), 2);
        assert!(h.translation.contains(&h.dictation[0]));
        assert!(h.translation.contains(&h.assistant[0]));
    }

    #[test]
    fn dictionary_terms_are_normalized_for_save() {
        let mut s = Settings::default();
        s.schema_version = 1;
        s.dictionary.terms = vec![
            " Typex ".into(),
            "".into(),
            "OpenAI".into(),
            "Typex".into(),
            "超长".repeat(40),
        ];
        s.normalize_for_save();
        assert_eq!(s.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(s.dictionary.terms[0], "Typex");
        assert_eq!(s.dictionary.terms[1], "OpenAI");
        assert_eq!(
            s.dictionary.terms[2].chars().count(),
            DICTIONARY_MAX_TERM_CHARS
        );
        assert_eq!(s.dictionary.terms.len(), 3);
    }

    #[test]
    fn dictionary_formats_stt_and_llm_context() {
        let dictionary = DictionarySettings {
            terms: vec!["Typex".into(), "A&B <tag>".into()],
        };
        let stt = dictionary.stt_prompt().unwrap();
        assert!(stt.contains("Typex"));
        assert!(stt.contains("A&B <tag>"));

        let llm = dictionary.llm_context().unwrap();
        assert!(llm.contains("- Typex"));
        assert!(llm.contains("A&amp;B &lt;tag&gt;"));
    }
}
