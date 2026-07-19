<script setup lang="ts">
// Canvas 波形（04 §6：60fps 仅录音时运行；80ms 电平平滑；处理中 = 五柱依次呼吸）
import { onMounted, onUnmounted, ref, watch } from "vue";
import { levelToVisualAmplitude, processingWavePhase } from "../../shared/waveform-scale";

const props = defineProps<{
  levels: number[];
  breathing?: boolean;
}>();

const canvas = ref<HTMLCanvasElement>();
const BAR_COUNT = 12;
const W = 96;
const H = 18;

let raf = 0;
let smoothed = new Array(BAR_COUNT).fill(0);
let target = new Array(BAR_COUNT).fill(0);
let running = false;
let breathingStartedAt: number | null = null;
let reducedMotion = false;

watch(
  () => props.levels,
  (l) => {
    if (!l.length) return;
    // 重采样到 BAR_COUNT
    for (let i = 0; i < BAR_COUNT; i++) {
      const v = l[Math.floor((i * l.length) / BAR_COUNT)] ?? 0;
      target[i] = levelToVisualAmplitude(v);
    }
    if (reducedMotion && !props.breathing) draw();
  },
);

watch(
  () => props.breathing,
  () => {
    breathingStartedAt = null;
    if (reducedMotion) draw(0);
  },
);

function draw(timestamp = 0) {
  const cv = canvas.value;
  if (!cv) return;
  const ctx = cv.getContext("2d")!;
  const dpr = window.devicePixelRatio || 1;
  ctx.clearRect(0, 0, W * dpr, H * dpr);
  const color = getComputedStyle(cv).getPropertyValue("--voice").trim() || "#171719";
  ctx.fillStyle = color;

  const barW = 3.5 * dpr;
  const gap = ((W - (BAR_COUNT * 3.5)) / (BAR_COUNT - 1)) * dpr;
  if (props.breathing && breathingStartedAt === null) breathingStartedAt = timestamp;
  const breathePhase = processingWavePhase(timestamp - (breathingStartedAt ?? timestamp));

  for (let i = 0; i < BAR_COUNT; i++) {
    let h: number;
    if (props.breathing) {
      // 处理中：依次呼吸（1.6s 循环，04 §6）
      const base = [6, 11, 17, 11, 6][i % 5] / 17;
      const phase = Math.sin(breathePhase - i * 0.5) * 0.3 + 0.7;
      h = base * phase * H * dpr;
      ctx.globalAlpha = phase;
    } else {
      // 电平 80ms 平滑
      smoothed[i] += (target[i] - smoothed[i]) * 0.25;
      h = Math.max(2 * dpr, smoothed[i] * H * dpr);
      ctx.globalAlpha = 1;
    }
    const x = i * (barW + gap);
    const y = (H * dpr - h) / 2;
    const r = barW / 2;
    ctx.beginPath();
    ctx.roundRect(x, y, barW, h, r);
    ctx.fill();
  }
  ctx.globalAlpha = 1;
  if (running) raf = requestAnimationFrame(draw);
}

function start() {
  if (running) return;
  running = true;
  reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (reducedMotion) {
    // 降级：所有状态均使用静态电平条（04 §6）
    running = false;
    draw(0);
    return;
  }
  raf = requestAnimationFrame(draw);
}

onMounted(() => {
  const cv = canvas.value!;
  const dpr = window.devicePixelRatio || 1;
  cv.width = W * dpr;
  cv.height = H * dpr;
  start();
});

onUnmounted(() => {
  running = false;
  cancelAnimationFrame(raf);
});
</script>

<template>
  <canvas ref="canvas" class="wave" :style="{ width: W + 'px', height: H + 'px' }" />
</template>

<style scoped>
.wave {
  display: block;
  color: var(--voice);
}
</style>
