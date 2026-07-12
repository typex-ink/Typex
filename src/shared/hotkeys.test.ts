import { describe, expect, it } from "vitest";
import {
  canonicalKeyId,
  deriveTranslationChord,
  hotkeyChordsAreReachable,
  keyIdFromKeyboardCode,
  keyIdFromKeyboardEvent,
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

  it("uses DOM location to correct WebView sided-modifier misreports", () => {
    expect(keyIdFromKeyboardEvent({ code: "ShiftLeft", location: 2 })).toBe("ShiftRight");
    expect(keyIdFromKeyboardEvent({ code: "ShiftRight", location: 1 })).toBe("ShiftLeft");
    expect(keyIdFromKeyboardEvent({ code: "Control", location: 2 })).toBe("ControlRight");
    expect(keyIdFromKeyboardEvent({ code: "KeyA", location: 2 })).toBe("KeyA");
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
    [[], ["AltRight"], ["ControlRight", "AltRight"]],
    [["ControlRight"], ["ControlRight"], ["ControlRight", "AltRight"]],
    [["ControlRight"], ["ControlRight", "KeyA"], ["ControlRight", "AltRight"]],
    [["AltGr", "KeyA"], ["AltRight"], ["ControlRight", "AltRight"]],
    [["ControlRight"], ["AltRight"], ["ControlRight"]],
    [["ControlRight"], ["AltRight"], []],
  ])("rejects empty or indistinguishable functional chords", (dictation, assistant, translation) => {
    expect(hotkeyChordsAreReachable(dictation, assistant, translation)).toBe(false);
  });

  it("allows shared keys when neither functional chord contains the other", () => {
    expect(
      hotkeyChordsAreReachable(
        ["ControlRight", "KeyA"],
        ["ControlRight", "KeyB"],
        ["ControlRight", "KeyA", "KeyB"],
      ),
    ).toBe(true);
  });

  it("allows an independent disjoint translation chord", () => {
    expect(
      hotkeyChordsAreReachable(
        ["ControlRight"],
        ["AltRight"],
        ["F13", "Menu"],
      ),
    ).toBe(true);
  });

  it("allows translation to be a strict subset of another chord", () => {
    expect(
      hotkeyChordsAreReachable(
        ["ControlRight", "KeyA"],
        ["AltRight"],
        ["ControlRight"],
      ),
    ).toBe(true);
  });
});
