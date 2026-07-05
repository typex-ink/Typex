<script setup lang="ts">
// 回答弹窗（05 §4 / ADR-23）：只读展示——指令回显 + 流式 Markdown 回答 + ✕ 关闭。
// 仅回答型结果经 assistant:// 事件到达此窗；改写型结果直接注入替换选区，不经过这里。
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import MarkdownIt from "markdown-it";
import { events } from "@/ipc/bindings";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";

const { t } = useI18n();

// LLM 输出视为不可信内容：禁 raw HTML（07 §11）
const md = new MarkdownIt({ html: false, linkify: true });

const instruction = ref("");
const selectionChars = ref<number | null>(null);
const answer = ref("");
const streaming = ref(false);
const errorText = ref("");
const panelEl = ref<HTMLElement | null>(null);
const currentRequest = ref(0);

const rendered = computed(() => md.render(answer.value));

// 流式渲染 30fps 节流（05 §4.3）
let pendingDelta = "";
let flushTimer: ReturnType<typeof setInterval> | null = null;
let resizeObserver: ResizeObserver | null = null;
let lastWindowHeight = 0;

const unlisteners: (() => void)[] = [];

onMounted(async () => {
  await syncWindowSize(true);

  unlisteners.push(
    await events.assistantStartedEvent.listen((e) => {
      // 新一轮提问：重置弹窗内容（单轮语义，05 §4.3）
      currentRequest.value = e.payload.request_id;
      instruction.value = e.payload.instruction;
      selectionChars.value = e.payload.selection_chars;
      answer.value = "";
      errorText.value = "";
      pendingDelta = "";
      streaming.value = true;
      syncWindowSize(true);
    }),
    await events.assistantDeltaEvent.listen((e) => {
      if (e.payload.request_id !== currentRequest.value) return; // 旧请求丢弃
      pendingDelta += e.payload.text_delta;
    }),
    await events.assistantDoneEvent.listen((e) => {
      if (e.payload.request_id !== currentRequest.value) return;
      flushDelta();
      answer.value = e.payload.full_text;
      streaming.value = false;
      syncWindowSize();
    }),
    await events.assistantErrorEvent.listen((e) => {
      if (e.payload.request_id !== currentRequest.value) return;
      streaming.value = false;
      errorText.value = e.payload.error.message;
      syncWindowSize();
    }),
  );

  flushTimer = setInterval(flushDelta, 33);
  window.addEventListener("keydown", onKey);
  if (panelEl.value) {
    resizeObserver = new ResizeObserver(() => syncWindowSize());
    resizeObserver.observe(panelEl.value);
  }

  // 焦点切换到其他应用 → 自动关闭（05 §4.1，无固定选项）
  const win = getCurrentWindow();
  unlisteners.push(
    await win.onFocusChanged(({ payload: focused }) => {
      if (!focused) close();
    }),
  );
});

onUnmounted(() => {
  unlisteners.forEach((u) => u());
  resizeObserver?.disconnect();
  if (flushTimer) clearInterval(flushTimer);
  window.removeEventListener("keydown", onKey);
});

function flushDelta() {
  if (pendingDelta) {
    answer.value += pendingDelta;
    pendingDelta = "";
    syncWindowSize();
  }
}

function close() {
  getCurrentWindow().hide();
}

async function syncWindowSize(force = false) {
  await nextTick();
  const panelRect = panelEl.value?.getBoundingClientRect();
  const height = panelRect ? Math.ceil(panelRect.height) : 96;
  if (!force && Math.abs(height - lastWindowHeight) < 2) return;
  lastWindowHeight = height;
  try {
    await getCurrentWindow().setSize(new LogicalSize(560, height));
  } catch {
    // 窗口隐藏/销毁过程中可能拒绝 resize；下一次显示会重新同步。
  }
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") close();
}
</script>

<template>
  <div class="panel-wrap">
    <div ref="panelEl" class="panel" data-tauri-drag-region>
      <!-- 指令回显行 + 关闭按钮 -->
      <div class="ask-row">
        <span class="ask">🎤 {{ instruction }}</span>
        <button class="x" :title="t('assistant.close')" @click="close">✕</button>
      </div>
      <div v-if="selectionChars !== null" class="chip-row">
        <span class="chip">{{ t("assistant.selection_chip", { n: selectionChars }) }}</span>
      </div>

      <!-- 回答区（流式 Markdown，文本可选中复制） -->
      <div v-if="answer || streaming" class="ans">
        <div class="bubble md" v-html="rendered" />
        <p v-if="streaming" class="streaming-hint">…</p>
      </div>
      <div v-if="errorText" class="ans">
        <p class="err">⚠ {{ errorText }}</p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.panel-wrap {
  width: 100vw;
  min-height: 0;
  box-sizing: border-box;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  background: transparent;
}
/* 面板：圆角 16 + 毛玻璃 + 边框，禁系统阴影（05 §4.1） */
.panel {
  box-sizing: border-box;
  width: 100vw;
  background: color-mix(in srgb, var(--surface) 88%, transparent);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border: 1px solid var(--border);
  border-radius: var(--radius-float);
  box-shadow: none;
  overflow: hidden;
}
@supports not (backdrop-filter: blur(20px)) {
  .panel {
    background: color-mix(in srgb, var(--surface) 98%, transparent);
  }
}
.ask-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  padding: 12px 14px 0;
}
.ask {
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--text-2);
  user-select: text;
}
.x {
  flex-shrink: 0;
  color: var(--text-3);
  cursor: pointer;
  background: none;
  border: none;
  font-size: 12px;
  padding: 2px 4px;
  border-radius: 6px;
}
.x:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: 1px;
}
.chip-row {
  display: flex;
  align-items: center;
  padding: 8px 14px 0;
}
.chip {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  font-size: 11.5px;
  color: var(--text-2);
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 99px;
  padding: 3px 10px;
}
.ans {
  padding: 12px 14px 14px;
  font-size: 13px;
  line-height: 1.6;
  max-height: 320px;
  overflow-y: auto;
}
.bubble {
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 10px 12px;
  user-select: text;
}
.md :deep(p) {
  margin: 0 0 8px;
}
.md :deep(p:last-child) {
  margin-bottom: 0;
}
.md :deep(pre),
.md :deep(code) {
  font-family: var(--font-mono);
  font-size: 11.5px;
  background: var(--surface-2);
  border: 1px solid var(--border);
  border-radius: 8px;
  color: var(--text-2);
}
.md :deep(pre) {
  padding: 8px 10px;
  overflow-x: auto;
  user-select: text;
}
.md :deep(code) {
  padding: 1px 4px;
}
.md :deep(pre code) {
  border: none;
  padding: 0;
}
.md :deep(ul),
.md :deep(ol) {
  padding-left: 20px;
  margin: 0 0 8px;
}
.streaming-hint {
  color: var(--text-3);
  margin-top: 6px;
}
.err {
  color: var(--error);
  font-size: 12.5px;
}
</style>
