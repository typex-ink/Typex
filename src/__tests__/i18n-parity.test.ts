// i18n 契约（CP-6.6）：zh-CN 与 en 全量 key 集合一致 + resolveLocale 行为
import { describe, expect, it, vi, afterEach } from "vitest";
import zhCN from "../i18n/zh-CN.json";
import en from "../i18n/en.json";
import { resolveLocale } from "../composables/useLocale";

function flattenKeys(obj: Record<string, unknown>, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([k, v]) =>
    v && typeof v === "object"
      ? flattenKeys(v as Record<string, unknown>, `${prefix}${k}.`)
      : [`${prefix}${k}`],
  );
}

describe("i18n 资源契约", () => {
  it("zh-CN 与 en 的 key 集合完全一致", () => {
    expect(flattenKeys(zhCN).sort()).toEqual(flattenKeys(en).sort());
  });

  it("没有空文案", () => {
    for (const messages of [zhCN, en] as Record<string, unknown>[]) {
      for (const key of flattenKeys(messages)) {
        const value = key.split(".").reduce<unknown>((o, k) => (o as Record<string, unknown>)[k], messages);
        expect(value, `空文案：${key}`).toBeTruthy();
      }
    }
  });
});

describe("resolveLocale", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("zh_cn → zh-CN，en → en", () => {
    expect(resolveLocale("zh_cn")).toBe("zh-CN");
    expect(resolveLocale("en")).toBe("en");
  });

  it("system 跟随 navigator.language", () => {
    vi.stubGlobal("navigator", { language: "zh-CN" });
    expect(resolveLocale("system")).toBe("zh-CN");
    expect(resolveLocale(undefined)).toBe("zh-CN");
    vi.stubGlobal("navigator", { language: "en-US" });
    expect(resolveLocale("system")).toBe("en");
  });
});
