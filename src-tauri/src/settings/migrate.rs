//! settings.json schema 迁移（07 §3.1）。

use crate::{
    settings::schema::CURRENT_SCHEMA_VERSION,
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
    normalize_hotkey_ids(&mut value);
    drop_legacy_keyring_credentials(&mut value);
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "schema_version".into(),
            Value::Number(CURRENT_SCHEMA_VERSION.into()),
        );
    }
    value
}

fn normalize_hotkey_ids(value: &mut Value) {
    let Some(hotkeys) = value.get_mut("hotkeys").and_then(Value::as_object_mut) else {
        return;
    };

    for field in ["dictation", "assistant"] {
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
        assert_eq!(migrated["schema_version"], 6);
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

        assert_eq!(migrated["schema_version"], 6);
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

        assert_eq!(migrated["schema_version"], 6);
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
    fn current_schema_still_normalizes_externally_edited_aliases() {
        let value = serde_json::json!({
            "schema_version": 6,
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
}
