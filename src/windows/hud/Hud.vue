<script setup lang="ts">
// HUD 状态胶囊 — 严格对照 docs/mockups/ui-mono.html §3 与 05 §3
// 极简纪律（07 §11）：无 Pinia、无路由、无 Markdown
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import {
  onSnapshot,
  onAudioLevel,
  sendCommand,
  cycleTranslationTarget,
  type SessionSnapshot,
  type ErrorCode,
} from "./ipc";
import Waveform from "./Waveform.vue";
// HUD 纪律：不引 vue-i18n 运行时，静态 JSON 按语言直取（文案仍单一来源）
import zhCN from "@/i18n/zh-CN.json";
import en from "@/i18n/en.json";

const L = navigator.language.toLowerCase().startsWith("zh") ? zhCN : en;

const snap = reactive<SessionSnapshot>({
  session_id: 0,
  mode: "dictation",
  phase: "idle",
  recording_ms: 0,
  verbatim: false,
  translation_direction: null,
  error: null,
  failed_stage: null,
  has_transcript: false,
  unpolished: false,
  processing_step: null,
});

const levels = ref<number[]>([]);
const elapsed = ref(0);
const processingSecs = ref(0);
const showSuccess = ref(false);
const silent = ref(false);

let timer: ReturnType<typeof setInterval> | null = null;
let lastVoice = 0;
let unlistenSnap: (() => void) | null = null;
let unlistenLevel: (() => void) | null = null;

// 错误文案（05 §9）：单一来源 = i18n 资源，key 与 Rust ErrorCode 对齐
const errorText: Record<ErrorCode, string> = L.error as Record<ErrorCode, string>;

const stageText: Record<string, string> = {
  transcribing: L.hud.stage_transcribing,
  processing: L.hud.stage_processing,
  injecting: L.hud.stage_injecting,
  recording: L.hud.stage_recording,
};

const modeLabel = computed(() => {
  if (snap.mode === "translation") return snap.translation_direction ?? "翻译";
  if (snap.mode === "assistant") return L.hud.mode_assistant;
  return snap.verbatim ? L.hud.mode_dictation_verbatim : L.hud.mode_dictation;
});

const processingText = computed(() => {
  if (snap.phase === "transcribing") return L.hud.transcribing;
  if (snap.mode === "translation") return L.hud.translating;
  if (snap.mode === "dictation") return snap.verbatim ? L.hud.injecting : L.hud.polishing;
  return L.hud.thinking;
});

const failText = computed(() => {
  const stage = stageText[snap.failed_stage ?? ""] ?? "";
  const msg = errorText[snap.error ?? "internal"];
  return snap.error === "no_focus" || snap.error === "no_speech"
    ? msg
    : stage
      ? `${stage}失败：${msg}`
      : msg;
});

const isRecording = computed(() => snap.phase === "recording");
const isProcessing = computed(
  () => snap.phase === "transcribing" || snap.phase === "processing" || snap.phase === "injecting",
);
const isFailed = computed(() => snap.phase === "failed");
// 胶囊是否可见：常驻单节点，阶段切换只换内容不重放出现动画（04 §6 动画仅出现/消失）
const active = computed(
  () => showSuccess.value || isRecording.value || isProcessing.value || isFailed.value,
);
// 翻译失败且转写在手 → 提供「注入原文」（02 F-2 降级）
const canInjectOriginal = computed(
  () => isFailed.value && snap.mode === "translation" && snap.has_transcript,
);
const canRetry = computed(() => isFailed.value && snap.error !== "no_focus");

function fmtTime(ms: number) {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

watch(
  () => snap.phase,
  (phase, prev) => {
    if (timer) clearInterval(timer);
    timer = null;
    if (phase === "recording") {
      elapsed.value = snap.recording_ms;
      silent.value = false;
      lastVoice = Date.now();
      timer = setInterval(() => {
        elapsed.value += 200;
        // 3 秒收不到声音 → 微提示（05 §3.2）
        silent.value = Date.now() - lastVoice > 3000;
      }, 200);
    } else if (phase === "transcribing" || phase === "processing") {
      processingSecs.value = 0;
      timer = setInterval(() => (processingSecs.value += 1), 1000);
    }
    // 注入完成（idle 且上一态非 failed/idle）→ 成功回弹 600ms（05 §3.2）
    if (phase === "idle" && prev === "injecting") {
      showSuccess.value = true;
      setTimeout(() => (showSuccess.value = false), 600);
    }
  },
);

onMounted(async () => {
  unlistenSnap = await onSnapshot((s) => Object.assign(snap, s));
  unlistenLevel = await onAudioLevel((l) => {
    levels.value = l;
    if (l.some((v) => v > 0.02)) lastVoice = Date.now();
  });
  window.addEventListener("keydown", onKey);
});

onUnmounted(() => {
  unlistenSnap?.();
  unlistenLevel?.();
  if (timer) clearInterval(timer);
  window.removeEventListener("keydown", onKey);
});

function onModeClick() {
  // 翻译模式下点徽标 = 目标语言快切（05 §3.2）
  if (snap.mode === "translation") {
    cycleTranslationTarget().then((next) => {
      if (snap.translation_direction) {
        const parts = snap.translation_direction.split("→");
        snap.translation_direction = `${parts[0]?.trim()} → ${next.slice(0, 2)}`;
      }
    });
  }
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") sendCommand(isRecording.value ? "cancel" : "dismiss");
}
</script>

<template>
  <div class="hud-viewport">
    <Transition name="hud">
      <!-- 单一常驻胶囊：阶段切换直接换内容，出现/消失才播放动画（防止每次状态切换闪一下） -->
      <div v-if="active" class="hud" :class="{ 'success-pop': showSuccess }">
        <!-- 成功反馈 -->
        <template v-if="showSuccess">
          <span class="ok">✓</span><span>{{ L.hud.injected }}</span>
        </template>

        <!-- 录音中 -->
        <template v-else-if="isRecording">
          <span class="dot" />
          <span class="time">{{ fmtTime(elapsed) }}</span>
          <span v-if="silent" class="hint">{{ L.hud.no_sound }}</span>
          <Waveform v-else :levels="levels" />
          <span
            class="mode"
            :class="{ clickable: snap.mode === 'translation' }"
            @click="onModeClick"
            >{{ modeLabel }}</span
          >
          <button class="x" title="取消（Esc）" @click="sendCommand('cancel')">✕</button>
        </template>

        <!-- 处理中 -->
        <template v-else-if="isProcessing">
          <Waveform :levels="[]" breathing />
          <span class="ptext">{{ processingText }}</span>
          <span v-if="processingSecs > 5" class="hint mono">{{ processingSecs }}s</span>
          <span v-if="snap.unpolished" class="hint">{{ L.hud.unpolished }}</span>
        </template>

        <!-- 失败（不自动消失，05 §3.2） -->
        <template v-else-if="isFailed">
          <span v-if="snap.error === 'no_focus'" class="info">ⓘ</span>
          <span v-else class="warn">⚠</span>
          <span class="ftext">{{ failText }}</span>
          <button v-if="canRetry" class="btn-sm" @click="sendCommand('retry')">
            {{ L.hud.retry }}
          </button>
          <button
            v-if="canInjectOriginal"
            class="btn-ghost-sm"
            @click="sendCommand('inject_original')"
          >
            {{ L.hud.inject_original }}
          </button>
          <button
            v-else-if="snap.has_transcript"
            class="btn-ghost-sm"
            @click="sendCommand('copy_transcript')"
          >
            {{ L.hud.copy_transcript }}
          </button>
          <button class="x" @click="sendCommand('dismiss')">✕</button>
        </template>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.hud-viewport {
  width: 100vw;
  height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  overflow: hidden;
}

/* 胶囊本体（mockup .hud）：高 44、全圆角、毛玻璃 */
.hud {
  display: inline-flex;
  align-items: center;
  gap: 9px;
  height: 44px;
  padding: 8px 16px 8px 13px;
  border-radius: var(--radius-hud);
  border: 1px solid var(--border);
  background: color-mix(in srgb, var(--surface) 82%, transparent);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  box-shadow: var(--shadow);
  font-size: 12.5px;
  color: var(--text-1);
  white-space: nowrap;
}

@supports not (backdrop-filter: blur(20px)) {
  /* 毛玻璃不可用（Linux/webkit2gtk）回退实心（04 §5） */
  .hud {
    background: color-mix(in srgb, var(--surface) 98%, transparent);
  }
}

/* 录音点：--recording，1s 呼吸（04 §3 唯一彩色） */
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--recording);
  animation: pulse 1.2s infinite;
  flex-shrink: 0;
}
@keyframes pulse {
  50% {
    opacity: 0.35;
  }
}

.time,
.mono {
  font-family: var(--font-mono);
  font-size: 11px;
}

.mode.clickable {
  cursor: pointer;
}
.mode {
  font-size: 10.5px;
  border: 1px solid var(--border-2);
  border-radius: 99px;
  padding: 1px 8px;
  color: var(--text-2);
}

.hint {
  color: var(--text-3);
  font-size: 11.5px;
}

.ptext {
  color: var(--text-2);
}

.ok {
  color: var(--success);
}
.warn {
  color: var(--error);
}
.info {
  color: var(--text-2);
}
.ftext {
  color: var(--text-1);
}

.x {
  color: var(--text-3);
  margin-left: 2px;
  cursor: pointer;
  background: none;
  border: none;
  font-size: 12px;
  padding: 2px 4px;
}
.x:hover {
  color: var(--text-1);
}

.btn-sm {
  height: 26px;
  font-size: 12px;
  padding: 0 10px;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-1);
  border: 1px solid var(--border-2);
  cursor: pointer;
}
.btn-sm:hover {
  background: var(--surface-2);
}

.btn-ghost-sm {
  height: 26px;
  font-size: 12px;
  padding: 0 6px;
  background: transparent;
  color: var(--text-2);
  border: none;
  text-decoration: underline;
  text-underline-offset: 3px;
  cursor: pointer;
}

/* 出现 220ms 上滑 12px + 淡入；消失 160ms 下滑淡出（04 §6） */
.hud-enter-active {
  transition: all 0.22s cubic-bezier(0.2, 0.9, 0.3, 1.2);
}
.hud-leave-active {
  transition: all 0.16s ease-in;
}
.hud-enter-from,
.hud-leave-to {
  opacity: 0;
  transform: translateY(12px);
}

/* 成功回弹 scale 1→1.04→1（04 §6） */
.success-pop {
  animation: pop 0.3s ease-out;
}
@keyframes pop {
  0% {
    transform: scale(1);
  }
  40% {
    transform: scale(1.04);
  }
  100% {
    transform: scale(1);
  }
}

@media (prefers-reduced-motion: reduce) {
  .dot {
    animation: none;
    opacity: 1;
  }
  .success-pop {
    animation: none;
  }
}
</style>
