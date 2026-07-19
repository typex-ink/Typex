<script setup lang="ts">
import { Check, Pause, Play } from "@lucide/vue";
import { computed, onMounted, onUnmounted, ref } from "vue";
import type { SiteCopy } from "../content";
import { DemoController, type DemoSnapshot } from "../demo-controller";
import DemoWaveform from "./DemoWaveform.vue";

const props = defineProps<{
  copy: SiteCopy["demo"];
}>();

const root = ref<HTMLElement>();
const reducedMotion = ref(false);
const snapshot = ref<DemoSnapshot>({
  phase: "recording",
  isPlaying: false,
  pausedByUser: false,
});

let controller: DemoController | undefined;
let observer: IntersectionObserver | undefined;

const statusText = computed(() => {
  if (snapshot.value.phase === "recording") return props.copy.listening;
  if (snapshot.value.phase === "processing") return props.copy.polishing;
  return props.copy.typed;
});

const editorText = computed(() =>
  snapshot.value.phase === "complete" ? props.copy.result : props.copy.draft,
);

function onVisibilityChange(): void {
  controller?.setPageVisible(document.visibilityState === "visible");
}

onMounted(() => {
  reducedMotion.value = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  controller = new DemoController({
    reducedMotion: reducedMotion.value,
    onChange(next) {
      snapshot.value = next;
    },
  });
  controller.setPageVisible(document.visibilityState === "visible");

  if ("IntersectionObserver" in window && root.value) {
    observer = new IntersectionObserver(
      (entries) => controller?.setViewportVisible(entries[0]?.isIntersecting ?? false),
      { threshold: 0.15 },
    );
    observer.observe(root.value);
  } else {
    controller.setViewportVisible(true);
  }

  document.addEventListener("visibilitychange", onVisibilityChange);
});

onUnmounted(() => {
  document.removeEventListener("visibilitychange", onVisibilityChange);
  observer?.disconnect();
  controller?.dispose();
});
</script>

<template>
  <section
    ref="root"
    class="product-demo"
    :data-phase="snapshot.phase"
    :data-playing="snapshot.isPlaying"
  >
    <div class="product-demo__toolbar">
      <div class="window-dots" aria-hidden="true"><i /><i /><i /></div>
      <span>{{ copy.editorTitle }}</span>
      <button
        v-if="!reducedMotion"
        class="demo-control"
        type="button"
        :aria-label="snapshot.pausedByUser ? copy.play : copy.pause"
        :title="snapshot.pausedByUser ? copy.play : copy.pause"
        @click="controller?.toggleUserPaused()"
      >
        <Play v-if="snapshot.pausedByUser" :size="17" fill="currentColor" aria-hidden="true" />
        <Pause v-else :size="17" fill="currentColor" aria-hidden="true" />
      </button>
    </div>

    <div class="product-demo__body">
      <aside aria-hidden="true">
        <span class="sidebar-line sidebar-line--strong" />
        <span class="sidebar-line" />
        <span class="sidebar-line" />
        <span class="sidebar-line sidebar-line--short" />
      </aside>
      <div class="editor-sheet">
        <p class="editor-kicker">TYPEX / 07</p>
        <h3>{{ copy.documentTitle }}</h3>
        <p class="editor-meta">16 JUL 2026&nbsp;&nbsp;·&nbsp;&nbsp;PROJECT NOTES</p>
        <p class="editor-copy" :class="{ 'editor-copy--complete': snapshot.phase === 'complete' }">
          {{ editorText }}<span v-if="snapshot.phase === 'recording'" class="text-caret" />
        </p>
        <span class="editor-rule" aria-hidden="true" />
        <span class="editor-rule editor-rule--short" aria-hidden="true" />
      </div>
    </div>

    <div class="demo-hud" :class="`demo-hud--${snapshot.phase}`">
      <template v-if="snapshot.phase === 'complete'">
        <Check :size="18" :stroke-width="2.5" aria-hidden="true" />
        <span class="hud-status">{{ statusText }}</span>
      </template>
      <template v-else>
        <span v-if="snapshot.phase === 'recording'" class="recording-dot" aria-hidden="true" />
        <span class="hud-status">{{ statusText }}</span>
        <DemoWaveform :phase="snapshot.phase" :active="snapshot.isPlaying" />
        <span class="hud-mode">{{ copy.mode }}</span>
      </template>
    </div>

    <span class="sr-only" aria-live="polite">{{ statusText }}</span>
  </section>
</template>
