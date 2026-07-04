// 设置字段双向绑定辅助：computed({get, set→store.mutate})
import { computed } from "vue";
import type { Settings } from "@/ipc/bindings";
import { useSettingsStore } from "@/stores/settings";

export function useSetting<T>(get: (s: Settings) => T, set: (s: Settings, v: T) => void) {
  const store = useSettingsStore();
  return computed<T>({
    get: () => get(store.settings!),
    set: (v) => void store.mutate((d) => set(d, v)),
  });
}
