<script setup lang="ts">
import {
  ArrowRight,
  Check,
  HardDrive,
  KeyRound,
  Languages,
  Mic,
  ShieldCheck,
  Sparkles,
} from "@lucide/vue";
import { onMounted, onUnmounted, ref } from "vue";
import type { FeatureCopy } from "../content";

defineProps<{
  feature: FeatureCopy;
}>();

const root = ref<HTMLElement>();
const animationActive = ref(false);

let observer: IntersectionObserver | undefined;
let motionQuery: MediaQueryList | undefined;
let inViewport = false;
let reducedMotion = false;

function syncAnimation(): void {
  animationActive.value =
    inViewport && document.visibilityState === "visible" && !reducedMotion;
}

function onVisibilityChange(): void {
  syncAnimation();
}

function onMotionChange(event: MediaQueryListEvent): void {
  reducedMotion = event.matches;
  syncAnimation();
}

onMounted(() => {
  motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  reducedMotion = motionQuery.matches;
  motionQuery.addEventListener("change", onMotionChange);

  if ("IntersectionObserver" in window && root.value) {
    observer = new IntersectionObserver(
      (entries) => {
        inViewport = entries[0]?.isIntersecting ?? false;
        syncAnimation();
      },
      { threshold: 0.25 },
    );
    observer.observe(root.value);
  } else {
    inViewport = true;
    syncAnimation();
  }

  document.addEventListener("visibilitychange", onVisibilityChange);
});

onUnmounted(() => {
  document.removeEventListener("visibilitychange", onVisibilityChange);
  motionQuery?.removeEventListener("change", onMotionChange);
  observer?.disconnect();
});
</script>

<template>
  <div
    ref="root"
    class="feature-visual"
    :class="[
      `feature-visual--${feature.kind}`,
      { 'feature-visual--active': animationActive },
    ]"
    aria-hidden="true"
  >
    <div class="visual-titlebar">
      <div class="window-dots"><i /><i /><i /></div>
      <span>{{ feature.visual.title }}</span>
      <Mic v-if="feature.kind === 'dictation'" :size="16" />
      <Languages v-else-if="feature.kind === 'translation'" :size="16" />
      <Sparkles v-else-if="feature.kind === 'assistant'" :size="16" />
      <ShieldCheck v-else :size="16" />
    </div>

    <div v-if="feature.kind === 'dictation'" class="dictation-scene">
      <div class="scene-row scene-row--muted">
        <span>{{ feature.visual.labelA }}</span>
        <p>{{ feature.visual.valueA }}</p>
      </div>
      <div class="scene-transform"><ArrowRight :size="18" /></div>
      <div class="scene-row scene-row--result">
        <span>{{ feature.visual.labelB }}</span>
        <p>{{ feature.visual.valueB }}<i class="text-caret" /></p>
      </div>
    </div>

    <div v-else-if="feature.kind === 'translation'" class="translation-scene">
      <div class="translation-panel translation-panel--source">
        <span class="scene-label">{{ feature.visual.labelA }}</span>
        <p>{{ feature.visual.valueA }}</p>
        <div class="mini-wave"><i /><i /><i /><i /><i /></div>
      </div>
      <div class="translation-arrow"><ArrowRight :size="22" /></div>
      <div class="translation-panel translation-panel--target">
        <span class="scene-label">{{ feature.visual.labelB }}</span>
        <p>{{ feature.visual.valueB }}</p>
        <span class="translation-caret" />
      </div>
    </div>

    <div v-else-if="feature.kind === 'assistant'" class="assistant-scene">
      <div class="selection-document">
        <span class="scene-label">{{ feature.visual.labelA }}</span>
        <p><mark>{{ feature.visual.valueA }}</mark></p>
        <span class="document-line" /><span class="document-line document-line--short" />
      </div>
      <div class="assistant-command">
        <Sparkles :size="17" />
        <div>
          <span>{{ feature.visual.labelB }}</span>
          <p>{{ feature.visual.valueB }}</p>
        </div>
      </div>
    </div>

    <div v-else class="local-scene">
      <div class="provider-row">
        <HardDrive :size="18" />
        <div><span>{{ feature.visual.labelA }}</span><strong>{{ feature.visual.valueA }}</strong></div>
        <Check :size="17" />
      </div>
      <div class="provider-row">
        <KeyRound :size="18" />
        <div><span>{{ feature.visual.labelB }}</span><strong>{{ feature.visual.valueB }}</strong></div>
        <Check :size="17" />
      </div>
    </div>

    <div class="visual-note"><Check :size="14" />{{ feature.visual.note }}</div>
  </div>
</template>
