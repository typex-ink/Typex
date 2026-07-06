<script setup lang="ts">
// 助手页（mockup 2.4）
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import FormRow from "@/components/FormRow.vue";
import Button from "@/components/Button.vue";
import { useSettingsStore } from "@/stores/settings";

const PROCESS_DEFAULT = `你是文本处理引擎。用户选中了一段文本并口述了处理要求。
若要求是对文本的加工（改写/翻译/精简/格式化等）：只输出加工后的文本本身，
不解释、不寒暄，结果将直接替换原文；
若要求实际上是就这段文本提问：以「ANSWER:」开头输出简洁回答。
【选中文本】{selection}
【处理要求】{instruction}`;

const ASK_DEFAULT = `你是 Typex 语音助手。用户通过语音提出一个问题，这是单轮问答。
回答应直接、简洁、可立即使用；默认使用用户提问的语言。
用户当前选中的内容作为上下文：{selection}
【问题】{instruction}`;

type PromptKind = "process" | "ask";

const PROMPTS = {
  process: {
    defaultTemplate: PROCESS_DEFAULT,
    labelKey: "settings.assistant.process_prompt_label",
    hintKey: "settings.assistant.process_prompt_hint",
    required: ["{selection}", "{instruction}"],
    rows: 8,
  },
  ask: {
    defaultTemplate: ASK_DEFAULT,
    labelKey: "settings.assistant.ask_prompt_label",
    hintKey: "settings.assistant.ask_prompt_hint",
    required: ["{instruction}"],
    rows: 6,
  },
} satisfies Record<
  PromptKind,
  {
    defaultTemplate: string;
    labelKey: string;
    hintKey: string;
    required: string[];
    rows: number;
  }
>;

const { t } = useI18n();
const store = useSettingsStore();

const promptOpen = ref<PromptKind | null>(null);
const draft = ref("");
const activePrompt = computed(() => (promptOpen.value ? PROMPTS[promptOpen.value] : PROMPTS.ask));
const missing = computed(() => activePrompt.value.required.filter((p) => !draft.value.includes(p)));

function openEditor(kind: PromptKind) {
  const assistant = store.settings!.assistant;
  draft.value =
    (kind === "process" ? assistant.process_prompt : assistant.ask_prompt) ||
    PROMPTS[kind].defaultTemplate;
  promptOpen.value = kind;
}
function toggleEditor(kind: PromptKind) {
  if (promptOpen.value === kind) {
    promptOpen.value = null;
    return;
  }
  openEditor(kind);
}
function save() {
  const kind = promptOpen.value;
  if (!kind || missing.value.length) return;
  const v = draft.value === PROMPTS[kind].defaultTemplate ? "" : draft.value;
  store.mutate((d) => {
    if (kind === "process") {
      d.assistant.process_prompt = v;
    } else {
      d.assistant.ask_prompt = v;
    }
  });
  promptOpen.value = null;
}
function restoreDefault() {
  if (!promptOpen.value) return;
  draft.value = PROMPTS[promptOpen.value].defaultTemplate;
}
</script>

<template>
  <div>
    <h5 class="page-title">{{ t("settings.nav_assistant") }}</h5>

    <FormRow
      :label="t('settings.assistant.process_prompt_label')"
      :hint="t('settings.assistant.process_prompt_hint')"
    >
      <Button variant="ghost" size="sm" @click="toggleEditor('process')">
        {{ t(promptOpen === "process" ? "prompt.collapse" : "prompt.expand") }}
      </Button>
    </FormRow>
    <template v-if="promptOpen === 'process'">
      <textarea v-model="draft" class="ta" :rows="activePrompt.rows" spellcheck="false" />
      <p v-if="missing.length" class="ph-error">
        {{ t("settings.assistant.ph_missing_list", { list: missing.join(" · ") }) }}
      </p>
      <div class="editor-actions">
        <Button variant="primary" size="sm" :disabled="missing.length > 0" @click="save">
          {{ t("actions.save") }}
        </Button>
        <Button size="sm" @click="restoreDefault">{{ t("actions.restore_default") }}</Button>
      </div>
    </template>

    <FormRow
      :label="t('settings.assistant.ask_prompt_label')"
      :hint="t('settings.assistant.ask_prompt_hint')"
    >
      <Button variant="ghost" size="sm" @click="toggleEditor('ask')">
        {{ t(promptOpen === "ask" ? "prompt.collapse" : "prompt.expand") }}
      </Button>
    </FormRow>
    <template v-if="promptOpen === 'ask'">
      <textarea v-model="draft" class="ta" :rows="activePrompt.rows" spellcheck="false" />
      <p v-if="missing.length" class="ph-error">
        {{ t("settings.assistant.ph_missing_list", { list: missing.join(" · ") }) }}
      </p>
      <div class="editor-actions">
        <Button variant="primary" size="sm" :disabled="missing.length > 0" @click="save">
          {{ t("actions.save") }}
        </Button>
        <Button size="sm" @click="restoreDefault">{{ t("actions.restore_default") }}</Button>
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
