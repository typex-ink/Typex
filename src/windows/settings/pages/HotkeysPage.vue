<script setup lang="ts">
// 快捷键页（05 §7）
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import Callout from "@/components/Callout.vue";
import FormRow from "@/components/FormRow.vue";
import HotkeyRecorder from "@/components/HotkeyRecorder.vue";
import {
  hotkeyChordsAreReachable,
  normalizeHotkeyChord,
} from "@/shared/hotkeys";
import { useSettingsStore } from "@/stores/settings";

const { t } = useI18n();
const store = useSettingsStore();
const validationError = ref(false);

type FunctionalHotkey = "dictation" | "assistant" | "translation";

function saveHotkey(slot: FunctionalHotkey, value: string[]) {
  const settings = store.settings;
  if (!settings) return;

  const normalized = normalizeHotkeyChord(value);
  const dictation = slot === "dictation" ? normalized : settings.hotkeys.dictation;
  const assistant = slot === "assistant" ? normalized : settings.hotkeys.assistant;
  const translation = slot === "translation" ? normalized : settings.hotkeys.translation;
  if (!hotkeyChordsAreReachable(dictation, assistant, translation)) {
    validationError.value = true;
    return;
  }

  validationError.value = false;
  void store.mutate((draft) => {
    draft.hotkeys[slot] = normalized;
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
const translation = computed({
  get: () => store.settings?.hotkeys.translation ?? [],
  set: (value: string[]) => saveHotkey("translation", value),
});
</script>

<template>
  <div>
    <h5 class="page-title">{{ t("settings.nav_hotkeys") }}</h5>
    <Callout v-if="validationError" variant="warn" class="validation-error">
      {{ t("settings.hotkeys.unreachable_chords") }}
    </Callout>
    <FormRow :label="t('settings.nav_dictation')">
      <HotkeyRecorder v-model="dictation" />
    </FormRow>
    <FormRow :label="t('settings.nav_assistant')">
      <HotkeyRecorder v-model="assistant" />
    </FormRow>
    <FormRow :label="t('settings.nav_translation')">
      <HotkeyRecorder v-model="translation" />
    </FormRow>
  </div>
</template>

<style scoped>
.page-title {
  font-size: 15px;
  margin-bottom: 14px;
  font-weight: 600;
}
.validation-error {
  margin-bottom: 12px;
}
</style>
