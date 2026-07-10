<script setup lang="ts">
// HotkeyRecorder（04 §7 / 05 §7）：录制快捷键专用控件。
// KeyboardEvent.code 提供布局无关的物理位置；存储前统一为稳定 KeyId。
import { onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Kbd from "@/components/Kbd.vue";
import Button from "@/components/Button.vue";
import Callout from "@/components/Callout.vue";
import { usePlatform } from "@/composables/usePlatform";
import { keyIdFromKeyboardCode, normalizeHotkeyChord } from "@/shared/hotkeys";

const { t, te } = useI18n();
const { keyLabel, hotkeyConflictKey } = usePlatform();
const model = defineModel<string[]>({ required: true });

const recording = ref(false);
const pressed = ref<Set<string>>(new Set());
const warningKey = ref<string | null>(null);

function labelOf(key: string) {
  return keyLabel(key, t, te);
}

function onKeyDown(e: KeyboardEvent) {
  e.preventDefault();
  e.stopPropagation();
  if (e.code === "Escape") {
    stop();
    return;
  }
  const keyId = keyIdFromKeyboardCode(e.code);
  if (!keyId) return;
  pressed.value = new Set([...pressed.value, keyId]);
}

function onKeyUp(e: KeyboardEvent) {
  e.preventDefault();
  e.stopPropagation();
  const released = keyIdFromKeyboardCode(e.code);
  if (!released || !pressed.value.has(released)) return;
  // 任一已录制键开始释放 = chord 录制完成。
  if (pressed.value.size > 0) {
    const keys = normalizeHotkeyChord([...pressed.value]);
    model.value = keys;
    warningKey.value = keys.map(hotkeyConflictKey).find((key): key is string => Boolean(key)) ?? null;
    stop();
  }
}

function start() {
  recording.value = true;
  pressed.value = new Set();
  warningKey.value = null;
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
