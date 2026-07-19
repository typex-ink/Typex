<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import {
  levelToVisualAmplitude,
  processingWavePhase,
} from "../../../src/shared/waveform-scale";
import type { DemoPhase } from "../demo-controller";

const props = defineProps<{
  phase: DemoPhase;
  active: boolean;
}>();

const canvas = ref<HTMLCanvasElement>();
const WIDTH = 144;
const HEIGHT = 24;
const BAR_COUNT = 18;
const speechPattern = [
  0.011, 0.024, 0.046, 0.019, 0.063, 0.031, 0.052, 0.015, 0.039,
  0.071, 0.028, 0.049, 0.018, 0.058, 0.034, 0.022, 0.044, 0.014,
];

let frame = 0;
let startedAt = 0;

function draw(timestamp = 0): void {
  const element = canvas.value;
  const context = element?.getContext("2d");
  if (!element || !context) return;

  const dpr = window.devicePixelRatio || 1;
  context.clearRect(0, 0, WIDTH * dpr, HEIGHT * dpr);
  context.fillStyle = getComputedStyle(element).getPropertyValue("--voice").trim();

  const barWidth = 4 * dpr;
  const gap = ((WIDTH - BAR_COUNT * 4) / (BAR_COUNT - 1)) * dpr;
  const elapsed = Math.max(0, timestamp - startedAt);
  const breathePhase = processingWavePhase(elapsed);

  for (let index = 0; index < BAR_COUNT; index += 1) {
    let amplitude = 0.28;
    let alpha = 1;

    if (props.phase === "recording") {
      const pulse = 0.78 + Math.sin(elapsed / 165 + index * 0.72) * 0.22;
      amplitude = levelToVisualAmplitude(speechPattern[index] ?? 0.01) * pulse;
    } else if (props.phase === "processing") {
      const base = [0.36, 0.62, 1, 0.62, 0.36][index % 5] ?? 0.5;
      alpha = Math.sin(breathePhase - index * 0.42) * 0.26 + 0.7;
      amplitude = base * alpha;
    } else {
      amplitude = [0.32, 0.5, 0.72, 0.5, 0.32][index % 5] ?? 0.4;
      alpha = 0.72;
    }

    const height = Math.max(3 * dpr, amplitude * HEIGHT * dpr);
    const x = index * (barWidth + gap);
    const y = (HEIGHT * dpr - height) / 2;
    context.globalAlpha = alpha;
    context.beginPath();
    context.roundRect(x, y, barWidth, height, barWidth / 2);
    context.fill();
  }
  context.globalAlpha = 1;
}

function loop(timestamp: number): void {
  draw(timestamp);
  if (props.active) frame = requestAnimationFrame(loop);
}

function syncAnimation(): void {
  cancelAnimationFrame(frame);
  frame = 0;
  startedAt = performance.now();
  draw(startedAt);
  if (props.active) frame = requestAnimationFrame(loop);
}

watch(() => [props.active, props.phase] as const, syncAnimation);

onMounted(() => {
  const element = canvas.value;
  if (!element) return;
  const dpr = window.devicePixelRatio || 1;
  element.width = WIDTH * dpr;
  element.height = HEIGHT * dpr;
  syncAnimation();
});

onUnmounted(() => cancelAnimationFrame(frame));
</script>

<template>
  <canvas
    ref="canvas"
    class="demo-waveform"
    :style="{ width: `${WIDTH}px`, height: `${HEIGHT}px` }"
    aria-hidden="true"
  />
</template>
