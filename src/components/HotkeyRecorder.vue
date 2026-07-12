<script setup lang="ts">
// HotkeyRecorder（04 §7 / 05 §7）：录制快捷键专用控件。
// KeyboardEvent.code 提供布局无关的物理位置；存储前统一为稳定 KeyId。
import { onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Kbd from "@/components/Kbd.vue";
import Button from "@/components/Button.vue";
import Callout from "@/components/Callout.vue";
import { usePlatform } from "@/composables/usePlatform";
import { keyIdFromKeyboardEvent, normalizeHotkeyChord } from "@/shared/hotkeys";

const { t, te } = useI18n();
const { keyLabel, hotkeyConflictKey } = usePlatform();
const model = defineModel<string[]>({ required: true });

const recording = ref(false);
const pressed = ref<Set<string>>(new Set());
const warningKey = ref<string | null>(null);
const SIDED_MODIFIER = /^(Shift|Control|Alt|Meta)(Left|Right)$/;
const RECOVERY_TIMEOUT_MS = 2_000;
let unidentifiedKeyDowns = 0;
let recoveryTimer: ReturnType<typeof setTimeout> | null = null;

function labelOf(key: string) {
  return keyLabel(key, t, te);
}

function matchReleasedKey(released: string): string | null {
  if (pressed.value.has(released)) return released;

  const family = SIDED_MODIFIER.exec(released)?.[1];
  if (!family) return null;
  const matches = [...pressed.value].filter((key) => SIDED_MODIFIER.exec(key)?.[1] === family);
  if (matches.length !== 1) return null;

  const pressedKey = matches[0];
  const reconciled = pressedKey.endsWith("Right") || released.endsWith("Right")
    ? `${family}Right`
    : pressedKey;
  if (reconciled !== pressedKey) {
    pressed.value = new Set([...pressed.value].map((key) => key === pressedKey ? reconciled : key));
  }
  return reconciled;
}

function clearRecoveryTimer() {
  if (recoveryTimer !== null) {
    clearTimeout(recoveryTimer);
    recoveryTimer = null;
  }
}

function scheduleRecoveryTimeout() {
  clearRecoveryTimer();
  recoveryTimer = setTimeout(stop, RECOVERY_TIMEOUT_MS);
}

function onKeyDown(e: KeyboardEvent) {
  e.preventDefault();
  e.stopPropagation();
  if (e.code === "Escape") {
    stop();
    return;
  }
  const keyId = keyIdFromKeyboardEvent(e);
  if (!keyId) {
    if (!e.repeat && (e.code === "Unidentified" || e.code === "Process")) {
      unidentifiedKeyDowns += 1;
    }
    return;
  }
  pressed.value = new Set([...pressed.value, keyId]);
}

function onKeyUp(e: KeyboardEvent) {
  e.preventDefault();
  e.stopPropagation();
  const released = keyIdFromKeyboardEvent(e);
  if (!released) return;
  if (!matchReleasedKey(released)) {
    if (unidentifiedKeyDowns === 0) return;
    // WebView2 can lose a modifier keydown while still reporting its keyup.
    unidentifiedKeyDowns -= 1;
    pressed.value = new Set([...pressed.value, released]);
  }
  clearRecoveryTimer();
  // Keep listening when another modifier is still physically held. Its
  // keydown may have been lost, while the later keyup can still recover it.
  if (e.ctrlKey || e.shiftKey || e.altKey || e.metaKey) {
    scheduleRecoveryTimeout();
    return;
  }

  // 最后一个修饰键释放，或普通键开始释放 = chord 录制完成。
  if (pressed.value.size > 0) {
    const keys = normalizeHotkeyChord([...pressed.value]);
    model.value = keys;
    warningKey.value = keys.map(hotkeyConflictKey).find((key): key is string => Boolean(key)) ?? null;
    stop();
  }
}

function start() {
  clearRecoveryTimer();
  unidentifiedKeyDowns = 0;
  recording.value = true;
  pressed.value = new Set();
  warningKey.value = null;
  window.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("keyup", onKeyUp, true);
  window.addEventListener("blur", stop);
}

function stop() {
  clearRecoveryTimer();
  unidentifiedKeyDowns = 0;
  recording.value = false;
  pressed.value = new Set();
  window.removeEventListener("keydown", onKeyDown, true);
  window.removeEventListener("keyup", onKeyUp, true);
  window.removeEventListener("blur", stop);
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
