<script setup lang="ts">
// 翻译页（mockup 2.3）
import { computed, ref } from "vue";
import FormRow from "@/components/FormRow.vue";
import Select from "@/components/Select.vue";
import Toggle from "@/components/Toggle.vue";
import Button from "@/components/Button.vue";
import { useSetting } from "@/composables/useSetting";
import { useSettingsStore } from "@/stores/settings";

const TRANSLATE_DEFAULT = `你是一个专业翻译引擎。输入是语音转写文本，先在心中还原说话者的真实意图
（忽略语气词、重复与中途改口），再将其从{source_language}翻译为{target_language}。
规则：只输出译文本身；不解释、不加引号、不加任何前后缀；
保留原文的段落、列表与换行结构；语气与正式程度与原文一致；
若原文已经是{bidirectional_target}，则翻译为{bidirectional_source}（双向翻译）。
【原文】{transcript}`;

const LANGS = [
  "中文（简体）",
  "中文（繁體）",
  "English",
  "日本語",
  "한국어",
  "Français",
  "Deutsch",
  "Español",
  "Русский",
].map((l) => ({ value: l, label: l }));

const store = useSettingsStore();
const source = useSetting(
  (s) => s.translation.source_language,
  (s, v) => (s.translation.source_language = v),
);
const target = useSetting(
  (s) => s.translation.target_language,
  (s, v) => (s.translation.target_language = v),
);
const bidirectional = useSetting(
  (s) => s.translation.bidirectional,
  (s, v) => (s.translation.bidirectional = v),
);

const promptOpen = ref(false);
const draft = ref("");
const missing = computed(() =>
  ["{transcript}", "{source_language}", "{target_language}"].filter((p) => !draft.value.includes(p)),
);
function openEditor() {
  draft.value = store.settings!.translation.translate_prompt || TRANSLATE_DEFAULT;
  promptOpen.value = true;
}
function save() {
  if (missing.value.length) return;
  const v = draft.value === TRANSLATE_DEFAULT ? "" : draft.value;
  store.mutate((d) => void (d.translation.translate_prompt = v));
  promptOpen.value = false;
}
</script>

<template>
  <div>
    <h5 class="page-title">翻译</h5>
    <FormRow label="源语言" hint="你说话使用的语言">
      <Select v-model="source" :options="LANGS" />
    </FormRow>
    <FormRow label="目标语言" hint="要翻译成的语言">
      <Select v-model="target" :options="LANGS" />
    </FormRow>
    <FormRow label="双向翻译" :hint="`检测到你说的是 ${target} 时，自动译回 ${source}`">
      <Toggle v-model="bidirectional" />
    </FormRow>

    <FormRow
      v-if="!promptOpen"
      label="翻译提示词（高级）"
      hint="占位符：{transcript} 原始转写 · {source_language} 源语言 · {target_language} 目标语言 · {bidirectional_source}/{bidirectional_target} 双向子句（开关关闭时该行省略）"
    >
      <Button variant="ghost" size="sm" @click="openEditor">展开编辑 ▾</Button>
    </FormRow>
    <template v-else>
      <FormRow label="翻译提示词（高级）">
        <Button variant="ghost" size="sm" @click="promptOpen = false">收起 ▴</Button>
      </FormRow>
      <textarea v-model="draft" class="ta" rows="8" spellcheck="false" />
      <p v-if="missing.length" class="ph-error">缺少必需占位符：{{ missing.join("、") }}</p>
      <div class="editor-actions">
        <Button variant="primary" size="sm" :disabled="missing.length > 0" @click="save">保存</Button>
        <Button size="sm" @click="draft = TRANSLATE_DEFAULT">恢复默认</Button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.page-title {
  font-size: 15px;
  margin-bottom: 14px;
  font-weight: 600;
}
.ta {
  width: 100%;
  border: 1px solid var(--border);
  background: var(--surface-2);
  border-radius: var(--radius-control);
  padding: 9px 11px;
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 1.7;
  color: var(--text-1);
  resize: vertical;
  margin-top: 4px;
  box-sizing: border-box;
}
.ph-error {
  font-size: 11px;
  color: var(--error);
  margin: 4px 0;
}
.editor-actions {
  display: flex;
  gap: 8px;
  margin: 6px 0 10px;
}
</style>
