// 界面语言（ADR-11）：general.language 变化时切换 vue-i18n locale。
// system = 跟随 navigator.language；zh_cn / en 显式指定。
import { watch, type WatchStopHandle } from "vue";
import type { I18n } from "vue-i18n";
import type { UiLanguage } from "@/ipc/bindings";
import { useSettingsStore } from "@/stores/settings";

export function resolveLocale(lang: UiLanguage | undefined): "zh-CN" | "en" {
  if (lang === "zh_cn") return "zh-CN";
  if (lang === "en") return "en";
  return navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

/** 订阅 settings store，把 general.language 同步到 i18n.global.locale（需在 pinia 安装后调用） */
export function syncLocale(i18n: I18n<any, any, any, string, false>): WatchStopHandle {
  const store = useSettingsStore();
  void store.load(); // 幂等：确保未加载设置的窗口（如 assistant）也能拿到语言
  return watch(
    () => store.settings?.general.language,
    (lang) => {
      i18n.global.locale.value = resolveLocale(lang);
    },
    { immediate: true },
  );
}
