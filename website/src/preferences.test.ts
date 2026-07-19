import { describe, expect, it } from "vitest";
import {
  LOCALE_STORAGE_KEY,
  THEME_STORAGE_KEY,
  detectLocale,
  hasStoredTheme,
  initialLocale,
  initialTheme,
  persistLocale,
  persistTheme,
  type StorageLike,
} from "./preferences";

class MemoryStorage implements StorageLike {
  readonly values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

describe("site preferences", () => {
  it("selects Chinese only when the primary browser language is Chinese", () => {
    expect(detectLocale(["zh-CN", "en-US"])).toBe("zh-CN");
    expect(detectLocale(["zh-Hant-TW"])).toBe("zh-CN");
    expect(detectLocale(["ja-JP", "zh-CN"])).toBe("en");
    expect(detectLocale(["fr-FR"])).toBe("en");
    expect(detectLocale([])).toBe("en");
  });

  it("persists and restores an explicit locale", () => {
    const storage = new MemoryStorage();
    expect(initialLocale(storage, ["zh-CN"])).toBe("zh-CN");
    persistLocale(storage, "en");
    expect(storage.values.get(LOCALE_STORAGE_KEY)).toBe("en");
    expect(initialLocale(storage, ["zh-CN"])).toBe("en");
  });

  it("follows the system theme until a valid explicit theme is stored", () => {
    const storage = new MemoryStorage();
    expect(initialTheme(storage, true)).toBe("dark");
    expect(initialTheme(storage, false)).toBe("light");
    expect(hasStoredTheme(storage)).toBe(false);

    persistTheme(storage, "dark");
    expect(storage.values.get(THEME_STORAGE_KEY)).toBe("dark");
    expect(initialTheme(storage, false)).toBe("dark");
    expect(hasStoredTheme(storage)).toBe(true);
  });

  it("ignores invalid and unavailable storage", () => {
    const invalid = new MemoryStorage();
    invalid.values.set(LOCALE_STORAGE_KEY, "de");
    invalid.values.set(THEME_STORAGE_KEY, "sepia");
    expect(initialLocale(invalid, ["en-US"])).toBe("en");
    expect(initialTheme(invalid, true)).toBe("dark");

    const blocked: StorageLike = {
      getItem() {
        throw new Error("blocked");
      },
      setItem() {
        throw new Error("blocked");
      },
    };
    expect(initialLocale(blocked, ["zh-CN"])).toBe("zh-CN");
    expect(() => persistLocale(blocked, "en")).not.toThrow();
    expect(() => persistTheme(blocked, "light")).not.toThrow();
  });
});
