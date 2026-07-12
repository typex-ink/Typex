//! Stable physical key identifiers shared by settings and hotkey backends.

use std::borrow::Cow;
use std::collections::HashSet;

/// Persisted physical key identifier.
///
/// Canonical names follow `KeyboardEvent.code` where possible. They describe a
/// physical key position, not the character produced by the active layout.
pub type KeyId = String;

/// Converts historical browser/rdev/Win32 names to the persisted KeyId contract.
/// Unknown future identifiers are preserved so newer configurations remain readable.
pub fn canonical_key_id(raw: &str) -> Cow<'_, str> {
    let id = raw.trim();
    match id {
        "Return" => Cow::Borrowed("Enter"),
        "KpReturn" | "NumpadReturn" => Cow::Borrowed("NumpadEnter"),
        "Num0" => Cow::Borrowed("Digit0"),
        "Num1" => Cow::Borrowed("Digit1"),
        "Num2" => Cow::Borrowed("Digit2"),
        "Num3" => Cow::Borrowed("Digit3"),
        "Num4" => Cow::Borrowed("Digit4"),
        "Num5" => Cow::Borrowed("Digit5"),
        "Num6" => Cow::Borrowed("Digit6"),
        "Num7" => Cow::Borrowed("Digit7"),
        "Num8" => Cow::Borrowed("Digit8"),
        "Num9" => Cow::Borrowed("Digit9"),
        "LeftArrow" => Cow::Borrowed("ArrowLeft"),
        "RightArrow" => Cow::Borrowed("ArrowRight"),
        "UpArrow" => Cow::Borrowed("ArrowUp"),
        "DownArrow" => Cow::Borrowed("ArrowDown"),
        "Alt" | "OptionLeft" => Cow::Borrowed("AltLeft"),
        "AltGr" | "RightAlt" | "OptionRight" => Cow::Borrowed("AltRight"),
        "Meta" | "Win" | "Super" | "Command" | "WinLeft" | "SuperLeft" | "CommandLeft"
        | "OSLeft" => Cow::Borrowed("MetaLeft"),
        "WinRight" | "SuperRight" | "CommandRight" | "OSRight" => Cow::Borrowed("MetaRight"),
        "ContextMenu" | "Apps" => Cow::Borrowed("Menu"),
        "SemiColon" => Cow::Borrowed("Semicolon"),
        "Dot" => Cow::Borrowed("Period"),
        "BackQuote" => Cow::Borrowed("Backquote"),
        "LeftBracket" => Cow::Borrowed("BracketLeft"),
        "RightBracket" => Cow::Borrowed("BracketRight"),
        "BackSlash" => Cow::Borrowed("Backslash"),
        "Kp0" => Cow::Borrowed("Numpad0"),
        "Kp1" => Cow::Borrowed("Numpad1"),
        "Kp2" => Cow::Borrowed("Numpad2"),
        "Kp3" => Cow::Borrowed("Numpad3"),
        "Kp4" => Cow::Borrowed("Numpad4"),
        "Kp5" => Cow::Borrowed("Numpad5"),
        "Kp6" => Cow::Borrowed("Numpad6"),
        "Kp7" => Cow::Borrowed("Numpad7"),
        "Kp8" => Cow::Borrowed("Numpad8"),
        "Kp9" => Cow::Borrowed("Numpad9"),
        "KpMinus" => Cow::Borrowed("NumpadSubtract"),
        "KpPlus" => Cow::Borrowed("NumpadAdd"),
        "KpMultiply" => Cow::Borrowed("NumpadMultiply"),
        "KpDivide" => Cow::Borrowed("NumpadDivide"),
        "KpDelete" | "NumpadDelete" => Cow::Borrowed("NumpadDecimal"),
        "Function" => Cow::Borrowed("Fn"),
        "Esc" => Cow::Borrowed("Escape"),
        "Del" => Cow::Borrowed("Delete"),
        "Spacebar" => Cow::Borrowed("Space"),
        _ => Cow::Borrowed(id),
    }
}

/// Canonicalizes and de-duplicates a chord while preserving press/display order.
pub fn normalize_hotkey_chord(keys: &[KeyId]) -> Vec<KeyId> {
    let mut seen = HashSet::with_capacity(keys.len());
    keys.iter()
        .filter_map(|key| {
            let canonical = canonical_key_id(key).into_owned();
            if canonical.is_empty() || !seen.insert(canonical.clone()) {
                None
            } else {
                Some(canonical)
            }
        })
        .collect()
}

/// Derives translation from the two complete functional chords.
pub fn derive_translation_chord(dictation: &[KeyId], assistant: &[KeyId]) -> Vec<KeyId> {
    if dictation.is_empty() || assistant.is_empty() {
        return Vec::new();
    }

    let mut merged = Vec::with_capacity(dictation.len() + assistant.len());
    merged.extend_from_slice(dictation);
    merged.extend_from_slice(assistant);
    normalize_hotkey_chord(&merged)
}

/// Functional chords must be non-empty and distinguishable at their exact shape.
pub fn hotkey_chords_are_reachable(
    dictation: &[KeyId],
    assistant: &[KeyId],
    translation: &[KeyId],
) -> bool {
    let dictation = normalize_hotkey_chord(dictation);
    let assistant = normalize_hotkey_chord(assistant);
    let translation = normalize_hotkey_chord(translation);
    if dictation.is_empty() || assistant.is_empty() || translation.is_empty() {
        return false;
    }

    let is_subset = |left: &[KeyId], right: &[KeyId]| {
        left.iter()
            .all(|key| right.iter().any(|other| other == key))
    };
    let same_chord =
        |left: &[KeyId], right: &[KeyId]| left.len() == right.len() && is_subset(left, right);
    !is_subset(&dictation, &assistant)
        && !is_subset(&assistant, &dictation)
        && !same_chord(&translation, &dictation)
        && !same_chord(&translation, &assistant)
}

/// Modifier keys do not auto-repeat and can use duplicate-down stale recovery.
pub fn supports_stale_release_recovery(key: &str) -> bool {
    matches!(
        canonical_key_id(key).as_ref(),
        "AltLeft"
            | "AltRight"
            | "ControlLeft"
            | "ControlRight"
            | "MetaLeft"
            | "MetaRight"
            | "ShiftLeft"
            | "ShiftRight"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_and_browser_aliases_share_one_key_id_table() {
        let cases = [
            ("Enter", "Enter"),
            ("Return", "Enter"),
            ("Digit1", "Digit1"),
            ("Num1", "Digit1"),
            ("ArrowLeft", "ArrowLeft"),
            ("LeftArrow", "ArrowLeft"),
            ("AltRight", "AltRight"),
            ("AltGr", "AltRight"),
            ("Alt", "AltLeft"),
            ("Win", "MetaLeft"),
            ("WinRight", "MetaRight"),
            ("ContextMenu", "Menu"),
            ("Menu", "Menu"),
            ("SemiColon", "Semicolon"),
            ("Dot", "Period"),
            ("BackQuote", "Backquote"),
            ("LeftBracket", "BracketLeft"),
            ("BackSlash", "Backslash"),
            ("Kp1", "Numpad1"),
            ("KpReturn", "NumpadEnter"),
            ("KpDelete", "NumpadDecimal"),
            ("KeyA", "KeyA"),
            ("F13", "F13"),
            ("F19", "F19"),
        ];

        for (input, expected) in cases {
            assert_eq!(canonical_key_id(input), expected, "input={input}");
        }
    }

    #[test]
    fn chord_normalization_preserves_order_and_derives_translation_union() {
        let dictation =
            normalize_hotkey_chord(&["ControlRight".into(), "Num1".into(), "Digit1".into()]);
        let assistant = normalize_hotkey_chord(&["AltGr".into(), "KeyA".into()]);

        assert_eq!(dictation, ["ControlRight", "Digit1"]);
        assert_eq!(assistant, ["AltRight", "KeyA"]);
        assert_eq!(
            derive_translation_chord(&dictation, &assistant),
            ["ControlRight", "Digit1", "AltRight", "KeyA"]
        );
    }

    #[test]
    fn translation_requires_two_non_empty_function_chords() {
        assert!(derive_translation_chord(&[], &["AltRight".into()]).is_empty());
        assert!(derive_translation_chord(&["ControlRight".into()], &[]).is_empty());
    }

    #[test]
    fn functional_chords_reject_empty_and_indistinguishable_bindings() {
        let translation = ["ControlRight".into(), "AltRight".into()];
        assert!(!hotkey_chords_are_reachable(
            &[],
            &["AltRight".into()],
            &translation
        ));
        assert!(!hotkey_chords_are_reachable(
            &["ControlRight".into()],
            &["ControlRight".into()],
            &translation
        ));
        assert!(!hotkey_chords_are_reachable(
            &["ControlRight".into()],
            &["ControlRight".into(), "KeyA".into()],
            &translation
        ));
        assert!(!hotkey_chords_are_reachable(
            &["AltGr".into(), "KeyA".into()],
            &["AltRight".into()],
            &translation
        ));
        assert!(!hotkey_chords_are_reachable(
            &["ControlRight".into()],
            &["AltRight".into()],
            &["ControlRight".into()]
        ));
        assert!(!hotkey_chords_are_reachable(
            &["ControlRight".into()],
            &["AltRight".into()],
            &[]
        ));
    }

    #[test]
    fn shared_non_subset_chords_remain_reachable() {
        assert!(hotkey_chords_are_reachable(
            &["ControlRight".into(), "KeyA".into()],
            &["ControlRight".into(), "KeyB".into()],
            &["ControlRight".into(), "KeyA".into(), "KeyB".into()]
        ));
    }

    #[test]
    fn independent_translation_chord_can_be_disjoint() {
        assert!(hotkey_chords_are_reachable(
            &["ControlRight".into()],
            &["AltRight".into()],
            &["F13".into(), "Menu".into()]
        ));
    }

    #[test]
    fn translation_can_be_a_strict_subset_or_superset() {
        assert!(hotkey_chords_are_reachable(
            &["ControlRight".into(), "KeyA".into()],
            &["AltRight".into()],
            &["ControlRight".into()]
        ));
        assert!(hotkey_chords_are_reachable(
            &["ControlRight".into()],
            &["AltRight".into()],
            &["ControlRight".into(), "AltRight".into()]
        ));
    }
}
