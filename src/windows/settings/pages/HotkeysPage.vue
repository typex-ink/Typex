<script setup lang="ts">
// 快捷键页（05 §7）
import { useI18n } from "vue-i18n";
import FormRow from "@/components/FormRow.vue";
import HotkeyRecorder from "@/components/HotkeyRecorder.vue";
import { useSetting } from "@/composables/useSetting";

const { t } = useI18n();

const dictation = useSetting(
  (s) => s.hotkeys.dictation,
  (s, v) => {
    s.hotkeys.dictation = v;
    s.hotkeys.translation = [...v, ...s.hotkeys.assistant];
  },
);
const assistant = useSetting(
  (s) => s.hotkeys.assistant,
  (s, v) => {
    s.hotkeys.assistant = v;
    s.hotkeys.translation = [...s.hotkeys.dictation, ...v];
  },
);
</script>

<template>
  <div>
    <h5 class="page-title">{{ t("settings.nav_hotkeys") }}</h5>
    <p class="desc">{{ t("settings.hotkeys.desc") }}</p>
    <FormRow :label="t('settings.nav_dictation')">
      <HotkeyRecorder v-model="dictation" />
    </FormRow>
    <FormRow :label="t('settings.nav_assistant')">
      <HotkeyRecorder v-model="assistant" />
    </FormRow>
    <FormRow :label="t('settings.nav_translation')" :hint="t('settings.hotkeys.translation_hint')">
      <span class="combo">
        <HotkeyRecorder :model-value="dictation" style="pointer-events: none" />
        <span class="plus">+</span>
        <HotkeyRecorder :model-value="assistant" style="pointer-events: none" />
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
