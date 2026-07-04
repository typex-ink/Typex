<script setup lang="ts">
// 助手页（mockup 2.4）
import { computed, ref } from "vue";
import FormRow from "@/components/FormRow.vue";
import Select from "@/components/Select.vue";
import Button from "@/components/Button.vue";
import { useSetting } from "@/composables/useSetting";
import { useSettingsStore } from "@/stores/settings";

const ASK_DEFAULT = `你是 Typex 语音助手。用户通过语音提出一个问题，这是单轮问答。
回答应直接、简洁、可立即使用；默认使用用户提问的语言。
用户当前选中的内容作为上下文：{selection}
【问题】{instruction}`;

const store = useSettingsStore();
const disposition = useSetting(
  (s) => s.assistant.disposition,
  (s, v) => (s.assistant.disposition = v),
);

const promptOpen = ref(false);
const draft = ref("");
const missing = computed(() => !draft.value.includes("{instruction}"));
function openEditor() {
  draft.value = store.settings!.assistant.ask_prompt || ASK_DEFAULT;
  promptOpen.value = true;
}
function save() {
  if (missing.value) return;
  const v = draft.value === ASK_DEFAULT ? "" : draft.value;
  store.mutate((d) => void (d.assistant.ask_prompt = v));
  promptOpen.value = false;
}
</script>

<template>
  <div>
    <h5 class="page-title">助手</h5>
    <FormRow label="改写结果处置">
      <Select
        v-model="disposition"
        :options="[
          { value: 'auto_replace', label: '自动替换选区' },
          { value: 'preview', label: '预览确认后替换' },
        ]"
      />
    </FormRow>
    <FormRow
      v-if="!promptOpen"
      label="问答提示词（高级）"
      hint="占位符：{instruction} 语音指令 · {selection} 选中文本（可选）"
    >
      <Button variant="ghost" size="sm" @click="openEditor">展开编辑 ▾</Button>
    </FormRow>
    <template v-else>
      <FormRow label="问答提示词（高级）">
        <Button variant="ghost" size="sm" @click="promptOpen = false">收起 ▴</Button>
      </FormRow>
      <textarea v-model="draft" class="ta" rows="6" spellcheck="false" />
      <p v-if="missing" class="ph-error">缺少必需占位符 {{ "{instruction}" }}</p>
      <div class="editor-actions">
        <Button variant="primary" size="sm" :disabled="missing" @click="save">保存</Button>
        <Button size="sm" @click="draft = ASK_DEFAULT">恢复默认</Button>
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
