<script setup lang="ts">
// 听写页（mockup 2.2 / 2.2b）：整理开关 + 提示词模板编辑器 + 注入方式 + 麦克风
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import FormRow from "@/components/FormRow.vue";
import Select from "@/components/Select.vue";
import Toggle from "@/components/Toggle.vue";
import Button from "@/components/Button.vue";
import { useSetting } from "@/composables/useSetting";
import { useSettingsStore } from "@/stores/settings";
import { events, commands } from "@/ipc/bindings";

const POLISH_DEFAULT = `你是语音转写的后处理引擎。输入是一段语音识别原始文本，输出整理后的文本。
规则：删除语气词与无意义重复；修复标点与断句；
识别说话人的自我修正（如「不对/应该是/我是说」），只保留最终意图；
将口述的格式指令（另起一段、列成清单）转为真实格式；
不增删信息、不改变语言、不替换用词——整理不是改写；
只输出结果本身。
以下专有名词按原样保留：{dictionary}
【原始转写】{transcript}`;

const { t } = useI18n();
const store = useSettingsStore();
const polishEnabled = useSetting(
  (s) => s.dictation.polish_enabled,
  (s, v) => (s.dictation.polish_enabled = v),
);
const injectMethod = useSetting(
  (s) => s.dictation.inject_method,
  (s, v) => (s.dictation.inject_method = v),
);
const escCancels = useSetting(
  (s) => s.dictation.esc_cancels,
  (s, v) => (s.dictation.esc_cancels = v),
);
const pasteDelay = useSetting(
  (s) => s.dictation.paste_delay_ms,
  (s, v) => (s.dictation.paste_delay_ms = v),
);
const microphone = useSetting(
  (s) => s.dictation.microphone,
  (s, v) => (s.dictation.microphone = v),
);
// 麦克风设备列表（cpal 枚举，CP-6.4）
const devices = ref<string[]>([]);
const deviceOptions = computed(() => [
  { value: "", label: t("settings.dictation.mic_default") },
  ...devices.value.map((d) => ({ value: d, label: d })),
]);

// 提示词编辑器（05 §5.2：缺必需占位符禁用保存 + 行内报错）
const promptOpen = ref(false);
const draft = ref("");
const missingPlaceholder = computed(() => !draft.value.includes("{transcript}"));
const dirty = computed(
  () => draft.value !== (store.settings!.dictation.polish_prompt || POLISH_DEFAULT),
);

function openEditor() {
  draft.value = store.settings!.dictation.polish_prompt || POLISH_DEFAULT;
  promptOpen.value = true;
}
function savePrompt() {
  if (missingPlaceholder.value) return;
  const v = draft.value === POLISH_DEFAULT ? "" : draft.value;
  store.mutate((d) => void (d.dictation.polish_prompt = v));
  promptOpen.value = false;
}
function restoreDefault() {
  draft.value = POLISH_DEFAULT;
}

// 电平预览
const levels = ref<number[]>([]);
let unlisten: (() => void) | null = null;
onMounted(async () => {
  devices.value = await commands.listAudioDevices();
  unlisten = await events.audioLevelEvent.listen((e) => (levels.value = e.payload));
});
onUnmounted(() => unlisten?.());
</script>

<template>
  <div>
    <h5 class="page-title">{{ t("settings.nav_dictation") }}</h5>
    <FormRow :label="t('settings.dictation.polish')" :hint="t('settings.dictation.polish_hint')">
      <Toggle v-model="polishEnabled" />
    </FormRow>

    <FormRow
      v-if="!promptOpen"
      :label="t('settings.dictation.prompt_label')"
      :hint="t('settings.dictation.prompt_hint')"
    >
      <Button variant="ghost" size="sm" @click="openEditor">{{ t("prompt.expand") }}</Button>
    </FormRow>
    <template v-else>
      <FormRow :label="t('settings.dictation.prompt_label')">
        <Button variant="ghost" size="sm" @click="promptOpen = false">{{ t("prompt.collapse") }}</Button>
      </FormRow>
      <textarea v-model="draft" class="ta" rows="9" spellcheck="false" />
      <p class="ph-hint">
        {{ t("settings.dictation.ph_help_prefix") }}<b class="mono">{{ "{transcript}" }}</b>
        {{ t("settings.dictation.ph_help_transcript") }} ·
        <span class="mono">{{ "{dictionary}" }}</span> {{ t("settings.dictation.ph_help_dictionary") }}
      </p>
      <p v-if="missingPlaceholder" class="ph-error">
        {{ t("settings.dictation.ph_missing", { ph: "{transcript}" }) }}
      </p>
      <div class="editor-actions">
        <Button variant="primary" size="sm" :disabled="missingPlaceholder || !dirty" @click="savePrompt">
          {{ t("actions.save") }}
        </Button>
        <Button size="sm" @click="restoreDefault">{{ t("actions.restore_default") }}</Button>
      </div>
    </template>

    <FormRow :label="t('settings.dictation.inject_method')">
      <Select
        v-model="injectMethod"
        :options="[
          { value: 'auto', label: t('settings.dictation.inject_auto') },
          { value: 'paste', label: t('settings.dictation.inject_paste') },
          { value: 'type_direct', label: t('settings.dictation.inject_type') },
        ]"
      />
    </FormRow>
    <FormRow :label="t('settings.dictation.paste_delay')">
      <input
        v-model.number="pasteDelay"
        type="range"
        min="10"
        max="300"
        step="10"
        class="slider"
      />
      <span class="mono delay-val">{{ pasteDelay }}ms</span>
    </FormRow>
    <FormRow :label="t('settings.dictation.esc_cancels')">
      <Toggle v-model="escCancels" />
    </FormRow>
    <FormRow :label="t('settings.dictation.microphone')">
      <Select v-model="microphone" :options="deviceOptions" />
    </FormRow>
    <FormRow :label="t('settings.dictation.level_preview')">
      <div class="bars">
        <i
          v-for="(l, i) in levels.length ? levels.slice(0, 5) : [0, 0, 0, 0, 0]"
          :key="i"
          :style="{ height: Math.max(3, Math.min(17, l * 60)) + 'px' }"
        />
      </div>
    </FormRow>
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
.ph-hint {
  font-size: 11px;
  color: var(--text-3);
  margin: 8px 0 4px;
}
.ph-error {
  font-size: 11px;
  color: var(--error);
  margin: 2px 0 6px;
}
.mono {
  font-family: var(--font-mono);
}
.editor-actions {
  display: flex;
  gap: 8px;
  margin: 6px 0 10px;
}
.slider {
  width: 120px;
  accent-color: var(--primary);
}
.delay-val {
  font-size: 11px;
  color: var(--text-3);
}
.bars {
  display: flex;
  align-items: center;
  gap: 3px;
  height: 18px;
}
.bars i {
  width: 3.5px;
  border-radius: 2px;
  background: var(--voice);
  display: block;
  transition: height 0.08s linear;
}
</style>
