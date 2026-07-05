<script setup lang="ts">
// 助手面板（05 §4 / mockup §5）：560px 浮窗、上下文芯片、流式 Markdown、动作行
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import MarkdownIt from "markdown-it";
import Button from "@/components/Button.vue";
import { commands, events, type AnswerKind } from "@/ipc/bindings";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";

// LLM 输出视为不可信内容：禁 raw HTML（07 §11）
const md = new MarkdownIt({ html: false, linkify: true });

const input = ref("");
const answer = ref("");
const answerDone = ref(false);
const answerKind = ref<AnswerKind | null>(null);
const errorText = ref("");
const streaming = ref(false);
const pinned = ref(false);
const selection = ref<string | null>(null);
const inputEl = ref<HTMLInputElement | null>(null);
const panelEl = ref<HTMLElement | null>(null);
const currentRequest = ref(0);
const replaced = ref(false);

const selectionChars = computed(() => selection.value?.length ?? 0);

const rendered = computed(() => md.render(answer.value));

// 流式渲染 30fps 节流（05 §4.3）
let pendingDelta = "";
let flushTimer: ReturnType<typeof setInterval> | null = null;
let resizeObserver: ResizeObserver | null = null;
let lastWindowHeight = 0;

const unlisteners: (() => void)[] = [];

onMounted(async () => {
  // 呼出时读取选中文本作为上下文（F-3b）
  selection.value = await commands.readSelectionContext();
  await syncWindowSize(true);
  await focusInput();

  unlisteners.push(
    await events.assistantDeltaEvent.listen((e) => {
      if (e.payload.request_id < currentRequest.value) return; // 旧请求丢弃
      currentRequest.value = e.payload.request_id;
      streaming.value = true;
      pendingDelta += e.payload.text_delta;
      syncWindowSize();
    }),
    await events.assistantDoneEvent.listen(async (e) => {
      if (e.payload.request_id < currentRequest.value) return;
      flushDelta();
      answer.value = e.payload.full_text;
      answerKind.value = e.payload.kind;
      answerDone.value = true;
      streaming.value = false;
      await syncWindowSize();
      // F-3a：改写型 + 自动替换设置 → 直接替换选区
      if (e.payload.kind === "rewrite" && selection.value) {
        const settings = await commands.getSettings();
        if (settings.assistant.disposition === "auto_replace") {
          await doAction("replace");
        }
      }
    }),
    await events.assistantErrorEvent.listen((e) => {
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

  // 失焦自动隐藏（05 §4.1；📌 固定时不隐藏）
  const win = getCurrentWindow();
  unlisteners.push(
    await win.onFocusChanged(async ({ payload: focused }) => {
      if (focused) {
        selection.value = await commands.readSelectionContext();
        await syncWindowSize(true);
        await focusInput();
      } else if (!pinned.value) {
        win.hide();
      }
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

async function submit() {
  const text = input.value.trim();
  if (!text || streaming.value) return;
  // 单轮语义：新提问清空上一条（05 §4.3）
  answer.value = "";
  answerDone.value = false;
  answerKind.value = null;
  errorText.value = "";
  replaced.value = false;
  input.value = "";
  await syncWindowSize();
  const r = await commands.askAssistant(text, selection.value !== null);
  if (r.status === "error") errorText.value = r.error.message;
}

async function doAction(action: "replace" | "insert" | "copy") {
  await commands.assistantAction(action, answer.value);
  if (action === "replace") {
    replaced.value = true;
    if (!pinned.value) setTimeout(() => getCurrentWindow().hide(), 800);
  }
}

function removeSelection() {
  selection.value = null;
  commands.clearSelectionContext();
  syncWindowSize();
  focusInput();
}

async function syncWindowSize(force = false) {
  await nextTick();
  const panelRect = panelEl.value?.getBoundingClientRect();
  const height = panelRect ? Math.ceil(panelRect.height) : 136;
  if (!force && Math.abs(height - lastWindowHeight) < 2) return;
  lastWindowHeight = height;
  try {
    await getCurrentWindow().setSize(new LogicalSize(560, height));
  } catch {
    // 窗口隐藏/销毁过程中可能拒绝 resize；下一次显示会重新同步。
  }
}

async function focusInput() {
  await nextTick();
  inputEl.value?.focus();
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") {
    getCurrentWindow().hide();
  } else if (e.key === "Enter" && !e.isComposing) {
    if (document.activeElement?.tagName === "INPUT") {
      submit();
    } else if (answerDone.value) {
      // ⏎ 主动作：有选区=替换，无=复制（05 §4.3）
      doAction(selection.value && answerKind.value === "rewrite" ? "replace" : "copy");
    }
  }
}
</script>

<template>
  <div class="panel-wrap">
    <div ref="panelEl" class="panel" data-tauri-drag-region>
      <!-- 上下文芯片 -->
      <div v-if="selection" class="chip-row">
        <span class="chip">
          ⌗ 选中内容 · {{ selectionChars }} 字
          <button
            class="x"
            title="移除上下文"
            tabindex="-1"
            @mousedown.prevent
            @click="removeSelection"
          >
            ✕
          </button>
        </span>
      </div>

      <!-- 输入行 -->
      <div class="in-row" :class="{ bordered: answer || errorText }">
        <input
          ref="inputEl"
          v-model="input"
          class="input"
          placeholder="按住 右⌥ 说话，或输入问题…"
          autofocus
          @keydown.enter="submit"
        />
        <button class="mic" title="按住全局助手键语音输入">🎙</button>
      </div>

      <!-- 回答区（流式 Markdown） -->
      <div v-if="answer || streaming" class="ans">
        <div class="bubble md" v-html="rendered" />
        <p v-if="streaming" class="streaming-hint">…</p>
      </div>
      <div v-if="errorText" class="ans">
        <p class="err">⚠ {{ errorText }}</p>
      </div>

      <!-- 动作行 -->
      <div v-if="answerDone" class="act">
        <template v-if="replaced">
          <span class="ok">✓ 已替换</span>
        </template>
        <template v-else>
          <Button
            v-if="selection && answerKind === 'rewrite'"
            variant="primary"
            @click="doAction('replace')"
            >替换选区 ⏎</Button
          >
          <Button @click="doAction('insert')">插入到光标</Button>
          <Button variant="ghost" @click="doAction('copy')">复制</Button>
        </template>
        <button class="pin" :class="{ on: pinned }" title="固定面板" @click="pinned = !pinned">
          📌
        </button>
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
/* 面板：圆角 16 + 毛玻璃 + shadow（05 §4.1） */
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
.chip-row {
  display: flex;
  align-items: center;
  padding: 10px 14px 0;
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
.chip .x {
  color: var(--text-3);
  cursor: pointer;
  background: none;
  border: none;
  font-size: 11px;
  padding: 0;
}
.in-row {
  display: flex;
  gap: 8px;
  align-items: center;
  padding: 10px 14px 12px;
}
.in-row.bordered {
  border-bottom: 1px solid var(--border);
}
.input {
  flex: 1;
  height: 32px;
  border-radius: var(--radius-control);
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text-1);
  padding: 0 10px;
  font-size: 13px;
  font-family: inherit;
}
.input::placeholder {
  color: var(--text-3);
}
.mic {
  height: 32px;
  width: 36px;
  border-radius: var(--radius-control);
  border: 1px solid var(--border-2);
  background: transparent;
  cursor: default;
  font-size: 14px;
}
.ans {
  padding: 14px;
  font-size: 13px;
  line-height: 1.6;
  max-height: 320px;
  overflow-y: auto;
}
.bubble {
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 10px 12px;
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
.act {
  display: flex;
  gap: 8px;
  align-items: center;
  padding: 12px 14px;
  border-top: 1px solid var(--border);
}
.ok {
  color: var(--success);
  font-size: 13px;
}
.pin {
  margin-left: auto;
  background: none;
  border: none;
  cursor: pointer;
  opacity: 0.4;
  font-size: 13px;
}
.pin.on {
  opacity: 1;
}
</style>
