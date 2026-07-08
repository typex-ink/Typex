// 契约测试（07 §5.2）：Rust 全部 ErrorCode 在 zh-CN 与 en 中都有文案。
// 这是编译期抓不到的缝隙——Rust 加错误码忘配文案时此测试红。
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import zhCN from "../i18n/zh-CN.json";
import en from "../i18n/en.json";

function errorCodesFromBindings(): string[] {
  const src = readFileSync(resolve(__dirname, "../ipc/bindings.ts"), "utf-8");
  // export type ErrorCode = "auth_error" | ...（联合含 doc 注释，截到下一个 export 为止）
  const m = src.match(/export type ErrorCode =([\s\S]*?)\nexport /);
  if (!m) throw new Error("bindings.ts 中找不到 ErrorCode 类型（先跑 pnpm gen:ipc）");
  return [...m[1].matchAll(/"([a-z_]+)"/g)].map((x) => x[1]);
}

describe("ErrorCode i18n 契约", () => {
  const codes = errorCodesFromBindings();

  it("从 bindings 提取到全部错误码", () => {
    expect(codes.length).toBeGreaterThanOrEqual(10);
    expect(codes).toContain("auth_error");
    expect(codes).toContain("no_speech");
  });

  it.each(codes)("zh-CN 有 error.%s", (code) => {
    expect((zhCN.error as Record<string, string>)[code], `zh-CN 缺 error.${code}`).toBeTruthy();
  });

  it.each(codes)("en 有 error.%s", (code) => {
    expect((en.error as Record<string, string>)[code], `en 缺 error.${code}`).toBeTruthy();
  });

  it("两语言错误码集合一致", () => {
    expect(Object.keys(zhCN.error).sort()).toEqual(Object.keys(en.error).sort());
  });
});
