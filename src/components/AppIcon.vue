<script setup lang="ts">
// App 图标（04 §2.2）：黑白随主题反转——浅色黑底白 glyph，深色白底黑 glyph
withDefaults(defineProps<{ size?: number }>(), { size: 88 });
</script>

<template>
  <div class="appicon" :style="{ width: size + 'px', height: size + 'px', borderRadius: size * 0.22 + 'px' }">
    <div class="bars" :style="{ gap: size * 0.057 + 'px', height: size * 0.39 + 'px' }">
      <i v-for="(h, idx) in [0.42, 0.68, 1, 0.68, 0.42]" :key="idx"
        :style="{ width: size * 0.068 + 'px', height: h * size * 0.39 + 'px' }" />
    </div>
    <div class="stem" :style="{
      width: size * 0.068 + 'px',
      height: size * 0.31 + 'px',
      marginTop: size * 0.068 + 'px',
    }" />
  </div>
</template>

<style scoped>
.appicon {
  background: var(--icon-bg, #000);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  box-shadow: var(--shadow);
  flex-shrink: 0;
}
/* 浅色模式黑底白，深色白底黑（tokens 没有 icon 变量时局部定义） */
:global(:root) {
  --icon-bg: #000000;
  --icon-fg: #ffffff;
}
:global([data-theme="dark"]) {
  --icon-bg: #ffffff;
  --icon-fg: #000000;
}
@media (prefers-color-scheme: dark) {
  :global(:root:not([data-theme="light"])) {
    --icon-bg: #ffffff;
    --icon-fg: #000000;
  }
}
.bars {
  display: flex;
  align-items: center;
}
.bars i {
  border-radius: 99px;
  background: var(--icon-fg, #fff);
  display: block;
}
.stem {
  background: var(--icon-fg, #fff);
  border-radius: 99px;
  position: relative;
}
.stem::after {
  content: "";
  position: absolute;
  bottom: -1px;
  left: -70%;
  right: -70%;
  height: 18%;
  background: var(--icon-fg, #fff);
  border-radius: 99px;
}
</style>
