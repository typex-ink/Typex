<script setup lang="ts">
// HotkeyRecorder（04 §7 / 05 §7）：录制快捷键专用控件。
// 浏览器 keydown 的 e.code 可捕获单个修饰键（MetaRight 等），映射到 rdev 键名存储。
import { onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Kbd from "@/components/Kbd.vue";
import Button from "@/components/Button.vue";
import Callout from "@/components/Callout.vue";

const { t, te } = useI18n();
const model = defineModel<string[]>({ required: true });

const recording = ref(false);
const pressed = ref<Set<string>>(new Set());
const warningKey = ref<string | null>(null);

// e.code → rdev Debug 键名（settings.json 存储形态）
const CODE_TO_RDEV: Record<string, string> = {
  MetaLeft: "MetaLeft",
  MetaRight: "MetaRight",
  AltLeft: "Alt",
  AltRight: "AltGr",
  ControlLeft: "ControlLeft",
  ControlRight: "ControlRight",
  ShiftLeft: "ShiftLeft",
  ShiftRight: "ShiftRight",
  CapsLock: "CapsLock",
  F13: "F13", F14: "F14", F15: "F15", F16: "F16", F17: "F17", F18: "F18", F19: "F19",
};

// rdev 键名 → 展示标签（macOS 惯例，keys.* i18n 资源）
function labelOf(key: string) {
  return te(`keys.${key}`) ? t(`keys.${key}`) : key;
}

// 已知冲突警告表（05 §7.1）：键名 → i18n key
const CONFLICTS: Record<string, string> = {
  CapsLock: "components.hotkey.conflict_capslock",
  MetaLeft: "components.hotkey.conflict_meta_left",
  Alt: "components.hotkey.conflict_alt",
};

function onKeyDown(e: KeyboardEvent) {
  e.preventDefault();
  e.stopPropagation();
  if (e.code === "Escape") {
    stop();
    return;
  }
  const rdevKey = CODE_TO_RDEV[e.code] ?? e.code;
  pressed.value = new Set([...pressed.value, rdevKey]);
}

function onKeyUp(e: KeyboardEvent) {
  e.preventDefault();
  // 全部松开 = 完成录制
  if (pressed.value.size > 0) {
    const keys = [...pressed.value];
    model.value = keys;
    warningKey.value = keys.map((k) => CONFLICTS[k]).find(Boolean) ?? null;
    stop();
  }
}

function start() {
  recording.value = true;
  pressed.value = new Set();
  window.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("keyup", onKeyUp, true);
}

function stop() {
  recording.value = false;
  window.removeEventListener("keydown", onKeyDown, true);
  window.removeEventListener("keyup", onKeyUp, true);
}

onUnmounted(stop);
</script>

<template>
  <span class="rec-wrap">
    <template v-for="(k, i) in model" :key="k">
      <span v-if="i > 0" class="plus">+</span>
      <Kbd>{{ labelOf(k) }}</Kbd>
    </template>
    <Button size="sm" @click="recording ? stop() : start()">
      {{ recording ? t("components.hotkey.press") : t("components.hotkey.change") }}
    </Button>
  </span>
  <Callout v-if="recording" icon="⌨" class="rec-hint">
    <b>{{ t("components.hotkey.recording") }}</b> {{ t("components.hotkey.recording_hint") }}
  </Callout>
  <Callout v-if="warningKey" variant="warn" class="rec-hint">{{ t(warningKey) }}</Callout>
</template>

<style scoped>
.rec-wrap {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.plus {
  color: var(--text-3);
}
.rec-hint {
  margin-top: 8px;
}
</style>
