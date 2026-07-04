<script setup lang="ts">
// 听写页（mockup 2.2 / 2.2b）：整理开关 + 提示词模板编辑器 + 注入方式 + 麦克风
import { computed, onMounted, onUnmounted, ref } from "vue";
import FormRow from "@/components/FormRow.vue";
import Select from "@/components/Select.vue";
import Toggle from "@/components/Toggle.vue";
import Button from "@/components/Button.vue";
import { useSetting } from "@/composables/useSetting";
import { useSettingsStore } from "@/stores/settings";
import { events } from "@/ipc/bindings";

const POLISH_DEFAULT = `你是语音转写的后处理引擎。输入是一段语音识别原始文本，输出整理后的文本。
规则：删除语气词与无意义重复；修复标点与断句；
识别说话人的自我修正（如「不对/应该是/我是说」），只保留最终意图；
将口述的格式指令（另起一段、列成清单）转为真实格式；
不增删信息、不改变语言、不替换用词——整理不是改写；
只输出结果本身。
以下专有名词按原样保留：{dictionary}
【原始转写】{transcript}`;

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
  unlisten = await events.audioLevelEvent.listen((e) => (levels.value = e.payload));
});
onUnmounted(() => unlisten?.());
</script>

<template>
  <div>
    <h5 class="page-title">听写</h5>
    <FormRow label="文本整理" hint="去语气词、修标点、保留改口后的最终意图">
      <Toggle v-model="polishEnabled" />
    </FormRow>

    <FormRow
      v-if="!promptOpen"
      label="整理提示词（高级）"
      hint="占位符：{transcript} 原始转写 · {dictionary} 个人词典"
    >
      <Button variant="ghost" size="sm" @click="openEditor">展开编辑 ▾</Button>
    </FormRow>
    <template v-else>
      <FormRow label="整理提示词（高级）">
        <Button variant="ghost" size="sm" @click="promptOpen = false">收起 ▴</Button>
      </FormRow>
      <textarea v-model="draft" class="ta" rows="9" spellcheck="false" />
      <p class="ph-hint">
        可用占位符：<b class="mono">{{ "{transcript}" }}</b> 原始转写文本（必需） ·
        <span class="mono">{{ "{dictionary}" }}</span> 个人词典（可选，F-10）
      </p>
      <p v-if="missingPlaceholder" class="ph-error">缺少必需占位符 {{ "{transcript}" }}，无法保存</p>
      <div class="editor-actions">
        <Button variant="primary" size="sm" :disabled="missingPlaceholder || !dirty" @click="savePrompt">
          保存
        </Button>
        <Button size="sm" @click="restoreDefault">恢复默认</Button>
      </div>
    </template>

    <FormRow label="注入方式">
      <Select
        v-model="injectMethod"
        :options="[
          { value: 'auto', label: '自动（推荐）' },
          { value: 'paste', label: '剪贴板粘贴' },
          { value: 'type_direct', label: '逐字输入' },
        ]"
      />
    </FormRow>
    <FormRow label="粘贴延迟">
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
    <FormRow label="Esc 取消录音">
      <Toggle v-model="escCancels" />
    </FormRow>
    <FormRow label="麦克风">
      <Select model-value="" :options="[{ value: '', label: '系统默认' }]" />
    </FormRow>
    <FormRow label="电平预览">
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
