// i18n（ADR-11：中英双语首发；key 与 Rust ErrorCode 对齐）
import { createI18n } from "vue-i18n";
import zhCN from "./zh-CN.json";
import en from "./en.json";

export function makeI18n(locale?: string) {
  const detected = locale ?? (navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en");
  return createI18n({
    legacy: false,
    locale: detected,
    fallbackLocale: "en",
    messages: { "zh-CN": zhCN, en },
  });
}
