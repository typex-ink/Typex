<script setup lang="ts">
// 快捷键页（mockup 2.10）
import FormRow from "@/components/FormRow.vue";
import HotkeyRecorder from "@/components/HotkeyRecorder.vue";
import { useSetting } from "@/composables/useSetting";

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
    <h5 class="page-title">快捷键</h5>
    <p class="desc">听写与助手键位可改；翻译 = 两键同按（自动跟随）。</p>
    <FormRow label="听写">
      <HotkeyRecorder v-model="dictation" />
    </FormRow>
    <FormRow label="助手">
      <HotkeyRecorder v-model="assistant" />
    </FormRow>
    <FormRow label="翻译" hint="听写键 + 助手键 同时按住">
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
