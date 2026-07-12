//! settings.json schema 迁移（07 §3.1）。

use crate::{
    settings::schema::{
        CURRENT_SCHEMA_VERSION, VAD_ENERGY_THRESHOLD_MAX, VAD_ENERGY_THRESHOLD_MIN,
        VAD_NEURAL_THRESHOLD_MAX, VAD_NEURAL_THRESHOLD_MIN,
    },
    types::{derive_translation_chord, normalize_hotkey_chord},
};
use serde_json::Value;

pub fn migrate(mut value: Value) -> Value {
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if version < 2 {
        migrate_v1_to_v2(&mut value);
    }
    if version < 7 {
        migrate_v6_to_v7(&mut value);
    } else {
        sanitize_v7_vad(&mut value);
    }
    normalize_hotkey_ids(&mut value, version < 8);
    drop_legacy_keyring_credentials(&mut value);
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "schema_version".into(),
            Value::Number(CURRENT_SCHEMA_VERSION.into()),
        );
    }
    value
}

fn default_vad_value() -> Value {
    serde_json::json!({
        "mode": "neural",
        "energy_threshold": 0.010,
        "neural_threshold": 0.50
    })
}

fn migrate_v6_to_v7(value: &mut Value) {
    let Some(root) = value.as_object_mut() else {
        return;
    };
    let dictation = root
        .entry("dictation")
        .or_insert_with(|| Value::Object(Default::default()));
    if let Some(dictation) = dictation.as_object_mut() {
        dictation.insert("vad".into(), default_vad_value());
    }
}

fn sanitize_v7_vad(value: &mut Value) {
    let Some(dictation) = value.get_mut("dictation").and_then(Value::as_object_mut) else {
        return;
    };
    let valid = dictation.get("vad").is_none_or(valid_vad_value);
    if !valid {
        tracing::warn!("检测到无效 VAD 配置，已仅恢复 VAD 默认值");
        dictation.insert("vad".into(), default_vad_value());
    }
}

fn valid_vad_value(value: &Value) -> bool {
    let Some(vad) = value.as_object() else {
        return false;
    };
    let mode_valid = matches!(
        vad.get("mode").and_then(Value::as_str),
        Some("energy" | "neural")
    );
    let energy_valid = vad
        .get("energy_threshold")
        .and_then(Value::as_f64)
        .is_some_and(|threshold| {
            threshold.is_finite()
                && (f64::from(VAD_ENERGY_THRESHOLD_MIN)..=f64::from(VAD_ENERGY_THRESHOLD_MAX))
                    .contains(&threshold)
        });
    let neural_valid = vad
        .get("neural_threshold")
        .and_then(Value::as_f64)
        .is_some_and(|threshold| {
            threshold.is_finite()
                && (f64::from(VAD_NEURAL_THRESHOLD_MIN)..=f64::from(VAD_NEURAL_THRESHOLD_MAX))
                    .contains(&threshold)
        });
    mode_valid && energy_valid && neural_valid
}

fn normalize_hotkey_ids(value: &mut Value, derive_legacy_translation: bool) {
    let Some(hotkeys) = value.get_mut("hotkeys").and_then(Value::as_object_mut) else {
        return;
    };

    for field in ["dictation", "assistant", "translation"] {
        let Some(keys) = hotkeys.get(field).and_then(Value::as_array) else {
            continue;
        };
        let keys = keys
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let normalized = normalize_hotkey_chord(&keys);
        hotkeys.insert(
            field.into(),
            Value::Array(normalized.into_iter().map(Value::String).collect()),
        );
    }

    let read_chord = |field: &str| -> Option<Vec<String>> {
        Some(
            hotkeys
                .get(field)?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        )
    };
    if derive_legacy_translation {
        let (Some(dictation), Some(assistant)) = (read_chord("dictation"), read_chord("assistant"))
        else {
            return;
        };
        let translation = derive_translation_chord(&dictation, &assistant);
        hotkeys.insert(
            "translation".into(),
            Value::Array(translation.into_iter().map(Value::String).collect()),
        );
    }
}

fn migrate_v1_to_v2(value: &mut Value) {
    let Some(profiles) = value.get_mut("profiles").and_then(Value::as_array_mut) else {
        return;
    };
    for profile in profiles {
        let Some(obj) = profile.as_object_mut() else {
            continue;
        };
        if obj.contains_key("capability") {
            obj.remove("slots");
            continue;
        }
        let capability = obj
            .get("slots")
            .and_then(Value::as_array)
            .map(|slots| {
                if slots.iter().any(|s| s.as_str() == Some("stt")) {
                    "stt"
                } else {
                    "llm"
                }
            })
            .unwrap_or_else(|| match obj.get("kind").and_then(Value::as_str) {
                Some("openai_compat" | "volcengine") => "stt",
                _ => "llm",
            });
        obj.insert("capability".into(), Value::String(capability.into()));
        obj.remove("slots");
    }
}

fn drop_legacy_keyring_credentials(value: &mut Value) {
    let Some(profiles) = value.get_mut("profiles").and_then(Value::as_array_mut) else {
        return;
    };
    for profile in profiles {
        let Some(credentials) = profile
            .get_mut("credentials")
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        credentials.retain(|_, value| {
            value
                .as_str()
                .is_none_or(|secret| !secret.trim().starts_with("keyring://"))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_slots_become_profile_capability() {
        let value = serde_json::json!({
            "schema_version": 1,
            "profiles": [
                { "id": "stt", "slots": ["stt"], "kind": "openai_compat" },
                { "id": "llm", "slots": ["polish", "translate"], "kind": "chat_completions" }
            ]
        });

        let migrated = migrate(value);
        assert_eq!(migrated["schema_version"], 8);
        assert_eq!(migrated["profiles"][0]["capability"], "stt");
        assert_eq!(migrated["profiles"][1]["capability"], "llm");
        assert!(migrated["profiles"][0].get("slots").is_none());
        assert!(migrated.get("dictionary").is_none());
    }

    #[test]
    fn v1_missing_slots_infers_from_kind() {
        let value = serde_json::json!({
            "schema_version": 1,
            "profiles": [
                { "id": "volc", "kind": "volcengine" },
                { "id": "chat", "kind": "responses" }
            ]
        });

        let migrated = migrate(value);
        assert_eq!(migrated["profiles"][0]["capability"], "stt");
        assert_eq!(migrated["profiles"][1]["capability"], "llm");
    }

    #[test]
    fn legacy_keyring_credentials_are_dropped() {
        let value = serde_json::json!({
            "schema_version": 2,
            "profiles": [
                {
                    "id": "llm",
                    "capability": "llm",
                    "credentials": {
                        "api_key": " keyring://typex/llm/llm/api_key ",
                        "other": "sk-plain"
                    }
                }
            ]
        });

        let migrated = migrate(value);

        assert!(
            migrated["profiles"][0]["credentials"]
                .get("api_key")
                .is_none()
        );
        assert_eq!(migrated["profiles"][0]["credentials"]["other"], "sk-plain");
    }

    #[test]
    fn v4_microphone_name_is_preserved_for_runtime_device_resolution() {
        let value = serde_json::json!({
            "schema_version": 4,
            "dictation": {
                "microphone": "USB Microphone"
            }
        });

        let migrated = migrate(value);

        assert_eq!(migrated["schema_version"], 8);
        assert_eq!(migrated["dictation"]["microphone"], "USB Microphone");
    }

    #[test]
    fn v5_hotkey_aliases_and_multi_key_chords_migrate_to_v6() {
        let value = serde_json::json!({
            "schema_version": 5,
            "hotkeys": {
                "dictation": ["ControlRight", "Num1", "Digit1"],
                "assistant": ["AltGr", "LeftArrow"],
                "translation": ["stale"]
            }
        });

        let migrated = migrate(value);

        assert_eq!(migrated["schema_version"], 8);
        assert_eq!(
            migrated["hotkeys"]["dictation"],
            serde_json::json!(["ControlRight", "Digit1"])
        );
        assert_eq!(
            migrated["hotkeys"]["assistant"],
            serde_json::json!(["AltRight", "ArrowLeft"])
        );
        assert_eq!(
            migrated["hotkeys"]["translation"],
            serde_json::json!(["ControlRight", "Digit1", "AltRight", "ArrowLeft"])
        );
    }

    #[test]
    fn v7_migration_derives_translation_with_the_legacy_rule() {
        let value = serde_json::json!({
            "schema_version": 7,
            "hotkeys": {
                "dictation": ["Return"],
                "assistant": ["ContextMenu"]
            }
        });

        let migrated = migrate(value);

        assert_eq!(
            migrated["hotkeys"]["dictation"],
            serde_json::json!(["Enter"])
        );
        assert_eq!(
            migrated["hotkeys"]["assistant"],
            serde_json::json!(["Menu"])
        );
        assert_eq!(
            migrated["hotkeys"]["translation"],
            serde_json::json!(["Enter", "Menu"])
        );
    }

    #[test]
    fn current_schema_normalizes_and_preserves_independent_translation() {
        let value = serde_json::json!({
            "schema_version": 8,
            "hotkeys": {
                "dictation": ["Return"],
                "assistant": ["ContextMenu"],
                "translation": ["AltGr", "Num1", "Digit1"]
            }
        });

        let migrated = migrate(value);

        assert_eq!(migrated["schema_version"], 8);
        assert_eq!(
            migrated["hotkeys"]["dictation"],
            serde_json::json!(["Enter"])
        );
        assert_eq!(
            migrated["hotkeys"]["assistant"],
            serde_json::json!(["Menu"])
        );
        assert_eq!(
            migrated["hotkeys"]["translation"],
            serde_json::json!(["AltRight", "Digit1"])
        );
    }

    #[test]
    fn v6_migrates_to_neural_vad_defaults() {
        let migrated = migrate(serde_json::json!({
            "schema_version": 6,
            "dictation": { "polish_enabled": false }
        }));

        assert_eq!(migrated["schema_version"], 8);
        assert_eq!(migrated["dictation"]["polish_enabled"], false);
        assert_eq!(migrated["dictation"]["vad"]["mode"], "neural");
        assert_eq!(migrated["dictation"]["vad"]["energy_threshold"], 0.010);
        assert_eq!(migrated["dictation"]["vad"]["neural_threshold"], 0.50);
    }

    #[test]
    fn invalid_v7_vad_only_restores_vad_subtree() {
        let migrated = migrate(serde_json::json!({
            "schema_version": 7,
            "general": { "autostart": false },
            "dictation": {
                "polish_enabled": false,
                "vad": {
                    "mode": "neural",
                    "energy_threshold": 0.0,
                    "neural_threshold": 0.5
                }
            }
        }));

        assert_eq!(migrated["general"]["autostart"], false);
        assert_eq!(migrated["dictation"]["polish_enabled"], false);
        assert_eq!(migrated["dictation"]["vad"]["mode"], "neural");
        assert_eq!(migrated["dictation"]["vad"]["energy_threshold"], 0.010);
    }
}
