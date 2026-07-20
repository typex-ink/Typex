//! SettingsService：读写、校验、变更广播（06 §5.1 / §9）。
pub mod migrate;
pub mod schema;

use crate::error::{ErrorCode, Result, TypexError};
use schema::Settings;
use std::path::PathBuf;
use tokio::sync::watch;

/// 设置服务：JSON 落盘 + watch 广播。
pub struct SettingsService {
    path: PathBuf,
    tx: watch::Sender<Settings>,
}

impl SettingsService {
    /// 从 `config_dir/settings.json` 加载；不存在或损坏时回退默认（损坏原文件保留 .bak）。
    pub fn load(config_dir: PathBuf) -> Self {
        let path = config_dir.join("settings.json");
        let settings = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text)
                .map(crate::settings::migrate::migrate)
                .and_then(serde_json::from_value::<Settings>)
                .map(|mut settings| {
                    settings.normalize_for_save();
                    if !settings.hotkeys.chords_are_reachable() {
                        tracing::warn!("检测到不可达快捷键配置，已恢复当前平台默认快捷键");
                        let defaults = schema::HotkeySettings::default();
                        settings.hotkeys.dictation = defaults.dictation;
                        settings.hotkeys.assistant = defaults.assistant;
                        settings.hotkeys.translation = defaults.translation;
                    }
                    settings
                }) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("settings.json 解析失败，回退默认: {e}");
                    let _ = std::fs::rename(&path, path.with_extension("json.bak"));
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        };
        let (tx, _) = watch::channel(settings);
        Self { path, tx }
    }

    pub fn get(&self) -> Settings {
        self.tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Settings> {
        self.tx.subscribe()
    }

    /// 全量替换并落盘 + 广播。
    pub fn update(&self, mut new: Settings) -> Result<Settings> {
        new.normalize_for_save();
        if !new.hotkeys.chords_are_reachable() {
            return Err(TypexError::new(
                ErrorCode::InvalidRequest,
                "三组快捷键必须非空且可区分",
            ));
        }
        if !new.dictation.vad.is_valid() {
            return Err(TypexError::new(
                ErrorCode::InvalidRequest,
                "VAD 门限必须是有限值且位于允许范围内",
            ));
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                TypexError::new(ErrorCode::Internal, format!("创建配置目录失败: {e}"))
            })?;
        }
        let json = serde_json::to_string_pretty(&new)
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("序列化设置失败: {e}")))?;
        std::fs::write(&self.path, json)
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("写入设置失败: {e}")))?;
        self.tx.send_replace(new.clone());
        Ok(new)
    }

    /// 就地修改（读-改-写）。
    pub fn mutate(&self, f: impl FnOnce(&mut Settings)) -> Result<Settings> {
        let mut s = self.get();
        f(&mut s);
        self.update(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_returns_default_and_update_persists() {
        let dir = std::env::temp_dir().join(format!("typex-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let svc = SettingsService::load(dir.clone());
        assert_eq!(svc.get().schema_version, schema::CURRENT_SCHEMA_VERSION);

        svc.mutate(|s| s.general.autostart = false).unwrap();
        let svc2 = SettingsService::load(dir.clone());
        assert!(!svc2.get().general.autostart);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_falls_back_to_default_with_bak() {
        let dir = std::env::temp_dir().join(format!("typex-test-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), "{ not json").unwrap();
        let svc = SettingsService::load(dir.clone());
        assert_eq!(svc.get().schema_version, schema::CURRENT_SCHEMA_VERSION);
        assert!(dir.join("settings.json.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_migrates_v1_profile_slots_to_capability() {
        let dir = std::env::temp_dir().join(format!("typex-test-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"
            {
              "schema_version": 1,
              "profiles": [
                {
                  "id": "old-llm",
                  "slots": ["polish", "translate"],
                  "kind": "chat_completions",
                  "label": "Old LLM",
                  "base_url": "https://api.example.com/v1",
                  "model": "m",
                  "credentials": {}
                }
              ]
            }
            "#,
        )
        .unwrap();

        let svc = SettingsService::load(dir.clone());
        let s = svc.get();
        assert_eq!(s.schema_version, schema::CURRENT_SCHEMA_VERSION);
        assert_eq!(
            s.profiles[0].capability,
            crate::types::ProviderCapability::Llm
        );
        assert_eq!(
            s.profiles[0].timeout_ms,
            crate::types::DEFAULT_PROVIDER_TIMEOUT_MS
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_v9_upgrades_legacy_timeout_and_preserves_custom_values() {
        let dir =
            std::env::temp_dir().join(format!("typex-test-timeout-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{
                "schema_version": 9,
                "profiles": [
                    {
                        "id": "legacy-default",
                        "capability": "llm",
                        "kind": "chat_completions",
                        "label": "Legacy default",
                        "model": "m",
                        "timeout_ms": 30000
                    },
                    {
                        "id": "custom",
                        "capability": "llm",
                        "kind": "chat_completions",
                        "label": "Custom",
                        "model": "m",
                        "timeout_ms": 45000
                    },
                    {
                        "id": "missing",
                        "capability": "llm",
                        "kind": "chat_completions",
                        "label": "Missing",
                        "model": "m"
                    }
                ]
            }"#,
        )
        .unwrap();

        let settings = SettingsService::load(dir.clone()).get();
        assert_eq!(settings.schema_version, schema::CURRENT_SCHEMA_VERSION);
        assert_eq!(
            settings
                .profiles
                .iter()
                .map(|profile| profile.timeout_ms)
                .collect::<Vec<_>>(),
            vec![
                crate::types::DEFAULT_PROVIDER_TIMEOUT_MS,
                45_000,
                crate::types::DEFAULT_PROVIDER_TIMEOUT_MS,
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_normalizes_v5_hotkey_aliases_and_derives_translation() {
        let dir =
            std::env::temp_dir().join(format!("typex-test-hotkey-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"
            {
              "schema_version": 5,
              "hotkeys": {
                "dictation": ["ControlRight", "Num1"],
                "assistant": ["AltGr", "KeyA"],
                "translation": ["stale"],
                "hold_threshold_ms": 350
              }
            }
            "#,
        )
        .unwrap();

        let settings = SettingsService::load(dir.clone()).get();
        assert_eq!(settings.schema_version, schema::CURRENT_SCHEMA_VERSION);
        assert_eq!(settings.hotkeys.dictation, ["ControlRight", "Digit1"]);
        assert_eq!(settings.hotkeys.assistant, ["AltRight", "KeyA"]);
        assert_eq!(
            settings.hotkeys.translation,
            ["ControlRight", "Digit1", "AltRight", "KeyA"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_preserves_independent_v8_translation_chord() {
        let dir = std::env::temp_dir().join(format!(
            "typex-test-independent-hotkey-load-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"
            {
              "schema_version": 8,
              "hotkeys": {
                "dictation": ["ControlRight"],
                "assistant": ["AltGr"],
                "translation": ["F13", "ContextMenu"],
                "hold_threshold_ms": 350
              }
            }
            "#,
        )
        .unwrap();

        let settings = SettingsService::load(dir.clone()).get();
        assert_eq!(settings.schema_version, schema::CURRENT_SCHEMA_VERSION);
        assert_eq!(settings.hotkeys.dictation, ["ControlRight"]);
        assert_eq!(settings.hotkeys.assistant, ["AltRight"]);
        assert_eq!(settings.hotkeys.translation, ["F13", "Menu"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_replaces_only_unreachable_hotkeys_with_platform_defaults() {
        let dir = std::env::temp_dir().join(format!(
            "typex-test-unreachable-hotkey-load-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"
            {
              "schema_version": 6,
              "general": { "autostart": false },
              "hotkeys": {
                "dictation": ["ControlRight"],
                "assistant": ["ControlRight", "KeyA"],
                "translation": ["stale"],
                "hold_threshold_ms": 999
              }
            }
            "#,
        )
        .unwrap();

        let settings = SettingsService::load(dir.clone()).get();
        assert!(!settings.general.autostart);
        let defaults = schema::HotkeySettings::default();
        assert_eq!(settings.hotkeys.dictation, defaults.dictation);
        assert_eq!(settings.hotkeys.assistant, defaults.assistant);
        assert_eq!(settings.hotkeys.translation, defaults.translation);
        assert_eq!(settings.hotkeys.hold_threshold_ms, 999);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_rejects_unreachable_hotkeys_without_changing_current_settings() {
        let dir = std::env::temp_dir().join(format!(
            "typex-test-unreachable-hotkey-update-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let service = SettingsService::load(dir.clone());
        let before = service.get();
        let mut invalid = before.clone();
        invalid.general.autostart = !before.general.autostart;
        invalid.hotkeys.assistant = invalid.hotkeys.dictation.clone();

        let error = service.update(invalid).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(service.get(), before);
        assert!(!dir.join("settings.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_rejects_translation_identical_to_dictation() {
        let dir = std::env::temp_dir().join(format!(
            "typex-test-shadowing-translation-update-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let service = SettingsService::load(dir.clone());
        let before = service.get();
        let mut invalid = before.clone();
        invalid.hotkeys.translation = invalid.hotkeys.dictation.clone();

        let error = service.update(invalid).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(service.get(), before);
        assert!(!dir.join("settings.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_allows_translation_as_a_strict_subset() {
        let dir = std::env::temp_dir().join(format!(
            "typex-test-subset-translation-update-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let service = SettingsService::load(dir.clone());
        let mut update = service.get();
        update.hotkeys.dictation = vec!["ControlRight".into(), "KeyA".into()];
        update.hotkeys.assistant = vec!["AltRight".into()];
        update.hotkeys.translation = vec!["ControlRight".into()];

        let saved = service.update(update.clone()).unwrap();
        assert_eq!(saved.hotkeys, update.hotkeys);
        assert_eq!(service.get().hotkeys, update.hotkeys);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_rejects_invalid_vad_without_changing_current_settings() {
        let dir = std::env::temp_dir().join(format!(
            "typex-test-invalid-vad-update-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let service = SettingsService::load(dir.clone());
        let before = service.get();
        let mut invalid = before.clone();
        invalid.general.autostart = !before.general.autostart;
        invalid.dictation.vad.neural_threshold = f32::NAN;

        let error = service.update(invalid).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(service.get(), before);
        assert!(!dir.join("settings.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_invalid_vad_preserves_other_settings() {
        let dir = std::env::temp_dir().join(format!(
            "typex-test-invalid-vad-load-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{
                "schema_version": 7,
                "general": { "autostart": false },
                "dictation": {
                    "polish_enabled": false,
                    "vad": {
                        "mode": "energy",
                        "energy_threshold": 0.2,
                        "neural_threshold": 0.5
                    }
                }
            }"#,
        )
        .unwrap();

        let settings = SettingsService::load(dir.clone()).get();
        assert!(!settings.general.autostart);
        assert!(!settings.dictation.polish_enabled);
        assert_eq!(settings.dictation.vad, schema::VadSettings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn watch_broadcasts_change() {
        let dir = std::env::temp_dir().join(format!("typex-test-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let svc = SettingsService::load(dir.clone());
        let mut rx = svc.subscribe();
        svc.mutate(|s| s.dictation.polish_enabled = false).unwrap();
        assert!(rx.has_changed().unwrap());
        assert!(!rx.borrow_and_update().dictation.polish_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
