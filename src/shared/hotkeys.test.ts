import { describe, expect, it } from "vitest";
import {
  canonicalKeyId,
  deriveTranslationChord,
  hotkeyChordsAreReachable,
  keyIdFromKeyboardCode,
  normalizeHotkeyChord,
} from "./hotkeys";

describe("stable hotkey KeyId contract", () => {
  it.each([
    ["Enter", "Enter"],
    ["Return", "Enter"],
    ["Digit1", "Digit1"],
    ["Num1", "Digit1"],
    ["ArrowLeft", "ArrowLeft"],
    ["LeftArrow", "ArrowLeft"],
    ["AltRight", "AltRight"],
    ["AltGr", "AltRight"],
    ["Alt", "AltLeft"],
    ["Win", "MetaLeft"],
    ["WinRight", "MetaRight"],
    ["ContextMenu", "Menu"],
    ["Menu", "Menu"],
    ["SemiColon", "Semicolon"],
    ["Dot", "Period"],
    ["BackQuote", "Backquote"],
    ["LeftBracket", "BracketLeft"],
    ["BackSlash", "Backslash"],
    ["Kp1", "Numpad1"],
    ["KpReturn", "NumpadEnter"],
    ["KpDelete", "NumpadDecimal"],
    ["KeyA", "KeyA"],
    ["F13", "F13"],
    ["F19", "F19"],
  ])("normalizes %s to %s", (input, expected) => {
    expect(canonicalKeyId(input)).toBe(expected);
  });

  it.each([
    ["Enter", "Enter"],
    ["Digit1", "Digit1"],
    ["ArrowLeft", "ArrowLeft"],
    ["AltRight", "AltRight"],
    ["ContextMenu", "Menu"],
    ["Semicolon", "Semicolon"],
    ["Period", "Period"],
    ["KeyZ", "KeyZ"],
    ["F13", "F13"],
    ["F19", "F19"],
  ])("maps browser code %s to %s", (code, expected) => {
    expect(keyIdFromKeyboardCode(code)).toBe(expected);
  });

  it("rejects non-physical browser placeholders", () => {
    expect(keyIdFromKeyboardCode("")).toBeNull();
    expect(keyIdFromKeyboardCode("Unidentified")).toBeNull();
    expect(keyIdFromKeyboardCode("Process")).toBeNull();
  });

  it("de-duplicates aliases and derives a multi-key translation union", () => {
    expect(normalizeHotkeyChord(["ControlRight", "Num1", "Digit1"])).toEqual([
      "ControlRight",
      "Digit1",
    ]);
    expect(
      deriveTranslationChord(
        ["ControlRight", "Num1"],
        ["AltGr", "KeyA", "AltRight"],
      ),
    ).toEqual(["ControlRight", "Digit1", "AltRight", "KeyA"]);
  });

  it.each([
    [[], ["AltRight"]],
    [["ControlRight"], ["ControlRight"]],
    [["ControlRight"], ["ControlRight", "KeyA"]],
    [["AltGr", "KeyA"], ["AltRight"]],
  ])("rejects empty, equal, or subset functional chords", (dictation, assistant) => {
    expect(hotkeyChordsAreReachable(dictation, assistant)).toBe(false);
  });

  it("allows shared keys when neither functional chord contains the other", () => {
    expect(
      hotkeyChordsAreReachable(
        ["ControlRight", "KeyA"],
        ["ControlRight", "KeyB"],
      ),
    ).toBe(true);
  });
});
