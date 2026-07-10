<script setup lang="ts">
// 快捷键页（05 §7）
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import Callout from "@/components/Callout.vue";
import FormRow from "@/components/FormRow.vue";
import HotkeyRecorder from "@/components/HotkeyRecorder.vue";
import {
  deriveTranslationChord,
  hotkeyChordsAreReachable,
  normalizeHotkeyChord,
} from "@/shared/hotkeys";
import { useSettingsStore } from "@/stores/settings";

const { t } = useI18n();
const store = useSettingsStore();
const validationError = ref(false);

type FunctionalHotkey = "dictation" | "assistant";

function saveHotkey(slot: FunctionalHotkey, value: string[]) {
  const settings = store.settings;
  if (!settings) return;

  const normalized = normalizeHotkeyChord(value);
  const dictation = slot === "dictation" ? normalized : settings.hotkeys.dictation;
  const assistant = slot === "assistant" ? normalized : settings.hotkeys.assistant;
  if (!hotkeyChordsAreReachable(dictation, assistant)) {
    validationError.value = true;
    return;
  }

  validationError.value = false;
  void store.mutate((draft) => {
    draft.hotkeys[slot] = normalized;
    draft.hotkeys.translation = deriveTranslationChord(dictation, assistant);
  });
}

const dictation = computed({
  get: () => store.settings?.hotkeys.dictation ?? [],
  set: (value: string[]) => saveHotkey("dictation", value),
});
const assistant = computed({
  get: () => store.settings?.hotkeys.assistant ?? [],
  set: (value: string[]) => saveHotkey("assistant", value),
});
</script>

<template>
  <div>
    <h5 class="page-title">{{ t("settings.nav_hotkeys") }}</h5>
    <p class="desc">{{ t("settings.hotkeys.desc") }}</p>
    <Callout v-if="validationError" variant="warn" class="validation-error">
      {{ t("settings.hotkeys.unreachable_chords") }}
    </Callout>
    <FormRow :label="t('settings.nav_dictation')">
      <HotkeyRecorder v-model="dictation" />
    </FormRow>
    <FormRow :label="t('settings.nav_assistant')">
      <HotkeyRecorder v-model="assistant" />
    </FormRow>
    <FormRow :label="t('settings.nav_translation')" :hint="t('settings.hotkeys.translation_hint')">
      <span class="combo">
        <HotkeyRecorder :model-value="dictation" />
        <span class="plus">+</span>
        <HotkeyRecorder :model-value="assistant" />
      </span>
    </FormRow>
  </div>
</template>

<style scoped>
.page-title {
  font-size: 15px;
  margin-bottom: 14px;
  font-weight: 600;
}
.desc {
  font-size: 12px;
  color: var(--text-2);
  margin: -10px 0 16px;
}
.validation-error {
  margin-bottom: 12px;
}
.combo {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.combo :deep(button) {
  display: none;
}
.plus {
  color: var(--text-3);
}
</style>
