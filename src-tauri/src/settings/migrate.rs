//! settings.json schema 迁移（08 §3.1）。

use crate::settings::schema::CURRENT_SCHEMA_VERSION;
use serde_json::Value;

pub fn migrate(mut value: Value) -> Value {
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if version < 2 {
        migrate_v1_to_v2(&mut value);
    }
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "schema_version".into(),
            Value::Number(CURRENT_SCHEMA_VERSION.into()),
        );
    }
    value
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
        assert_eq!(migrated["schema_version"], 2);
        assert_eq!(migrated["profiles"][0]["capability"], "stt");
        assert_eq!(migrated["profiles"][1]["capability"], "llm");
        assert!(migrated["profiles"][0].get("slots").is_none());
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
}
