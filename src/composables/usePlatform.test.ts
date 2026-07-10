import { beforeEach, describe, expect, it, vi } from "vitest";

const platformMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: platformMock,
}));

async function loadPlatform(platform: "macos" | "windows" | "linux") {
  platformMock.mockReturnValue(platform);
  vi.resetModules();
  return import("./usePlatform");
}

describe("usePlatform", () => {
  beforeEach(() => {
    platformMock.mockReset();
  });

  it("uses Windows defaults and key labels", async () => {
    const { usePlatform } = await loadPlatform("windows");
    const platform = usePlatform();
    const labels: Record<string, string> = {
      "keys_windows.ControlRight": "Right Ctrl",
      "keys_windows.AltRight": "Right Alt",
    };
    const t = (key: string) => labels[key] ?? key;
    const te = (key: string) => Object.hasOwn(labels, key);

    expect(platform.isWindows.value).toBe(true);
    expect(platform.defaultHotkeys.value).toEqual({
      dictation: ["ControlRight"],
      assistant: ["AltRight"],
      translation: ["ControlRight", "AltRight"],
    });
    expect(platform.keyLabel("ControlRight", t, te)).toBe("Right Ctrl");
    expect(platform.keyLabel("AltRight", t, te)).toBe("Right Alt");
    expect(platform.keyLabel("AltGr", t, te)).toBe("Right Alt");
    expect(platform.keyLabel("KeyA", t, te)).toBe("A");
    expect(platform.keyLabel("Digit1", t, te)).toBe("1");
    expect(platform.keyLabel("LeftArrow", t, te)).toBe("←");
    expect(platform.keyLabel("SemiColon", t, te)).toBe(";");
    expect(platform.hotkeyConflictKey("AltLeft")).toBe(
      "components.hotkey.conflict_alt_windows",
    );
    expect(platform.hotkeyConflictKey("Alt")).toBe(
      "components.hotkey.conflict_alt_windows",
    );
  });

  it("keeps macOS defaults isolated from Windows", async () => {
    const { usePlatform } = await loadPlatform("macos");
    const platform = usePlatform();

    expect(platform.isMacOS.value).toBe(true);
    expect(platform.defaultHotkeys.value.dictation).toEqual(["MetaRight"]);
    expect(platform.hotkeyConflictKey("AltLeft")).toBe("components.hotkey.conflict_alt");
  });
});
