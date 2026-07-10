/** Stable physical KeyId contract shared by browser recording and display. */
const KEY_ID_ALIASES: Readonly<Record<string, string>> = {
  Return: "Enter",
  KpReturn: "NumpadEnter",
  NumpadReturn: "NumpadEnter",
  Num0: "Digit0",
  Num1: "Digit1",
  Num2: "Digit2",
  Num3: "Digit3",
  Num4: "Digit4",
  Num5: "Digit5",
  Num6: "Digit6",
  Num7: "Digit7",
  Num8: "Digit8",
  Num9: "Digit9",
  LeftArrow: "ArrowLeft",
  RightArrow: "ArrowRight",
  UpArrow: "ArrowUp",
  DownArrow: "ArrowDown",
  Alt: "AltLeft",
  OptionLeft: "AltLeft",
  AltGr: "AltRight",
  RightAlt: "AltRight",
  OptionRight: "AltRight",
  Meta: "MetaLeft",
  Win: "MetaLeft",
  Super: "MetaLeft",
  Command: "MetaLeft",
  WinLeft: "MetaLeft",
  SuperLeft: "MetaLeft",
  CommandLeft: "MetaLeft",
  OSLeft: "MetaLeft",
  WinRight: "MetaRight",
  SuperRight: "MetaRight",
  CommandRight: "MetaRight",
  OSRight: "MetaRight",
  ContextMenu: "Menu",
  Apps: "Menu",
  SemiColon: "Semicolon",
  Dot: "Period",
  BackQuote: "Backquote",
  LeftBracket: "BracketLeft",
  RightBracket: "BracketRight",
  BackSlash: "Backslash",
  Kp0: "Numpad0",
  Kp1: "Numpad1",
  Kp2: "Numpad2",
  Kp3: "Numpad3",
  Kp4: "Numpad4",
  Kp5: "Numpad5",
  Kp6: "Numpad6",
  Kp7: "Numpad7",
  Kp8: "Numpad8",
  Kp9: "Numpad9",
  KpMinus: "NumpadSubtract",
  KpPlus: "NumpadAdd",
  KpMultiply: "NumpadMultiply",
  KpDivide: "NumpadDivide",
  KpDelete: "NumpadDecimal",
  NumpadDelete: "NumpadDecimal",
  Function: "Fn",
  Esc: "Escape",
  Del: "Delete",
  Spacebar: "Space",
};

export function canonicalKeyId(raw: string): string {
  const key = raw.trim();
  return KEY_ID_ALIASES[key] ?? key;
}

/** KeyboardEvent.code is physical-position based and is never replaced with event.key. */
export function keyIdFromKeyboardCode(code: string): string | null {
  if (!code || code === "Unidentified" || code === "Process") return null;
  return canonicalKeyId(code);
}

export function normalizeHotkeyChord(keys: readonly string[]): string[] {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const raw of keys) {
    const key = canonicalKeyId(raw);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    normalized.push(key);
  }
  return normalized;
}

export function deriveTranslationChord(
  dictation: readonly string[],
  assistant: readonly string[],
): string[] {
  const normalizedDictation = normalizeHotkeyChord(dictation);
  const normalizedAssistant = normalizeHotkeyChord(assistant);
  if (normalizedDictation.length === 0 || normalizedAssistant.length === 0) return [];
  return normalizeHotkeyChord([...normalizedDictation, ...normalizedAssistant]);
}

export function hotkeyChordsAreReachable(
  dictation: readonly string[],
  assistant: readonly string[],
): boolean {
  const normalizedDictation = normalizeHotkeyChord(dictation);
  const normalizedAssistant = normalizeHotkeyChord(assistant);
  if (normalizedDictation.length === 0 || normalizedAssistant.length === 0) return false;

  const isSubset = (left: readonly string[], right: readonly string[]) =>
    left.every((key) => right.includes(key));
  return (
    !isSubset(normalizedDictation, normalizedAssistant) &&
    !isSubset(normalizedAssistant, normalizedDictation)
  );
}
