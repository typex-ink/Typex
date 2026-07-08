// useSettings：Rust 是设置真身，Pinia 只是订阅缓存（06 §11 状态所有权）
import { defineStore } from "pinia";
import { ref } from "vue";
import { commands, events, type Settings } from "@/ipc/bindings";

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<Settings | null>(null);
  const loaded = ref(false);

  async function load() {
    settings.value = await commands.getSettings();
    if (!loaded.value) {
      loaded.value = true;
      await events.settingsChangedEvent.listen((e) => {
        settings.value = e.payload;
      });
    }
  }

  /// 就地修改并推送到 Rust（返回后 Rust 广播新配置回来）
  async function mutate(fn: (s: Settings) => void) {
    if (!settings.value) return;
    const draft = JSON.parse(JSON.stringify(settings.value)) as Settings;
    fn(draft);
    const result = await commands.updateSettings(draft);
    if (result.status === "ok") settings.value = result.data;
  }

  return { settings, loaded, load, mutate };
});
