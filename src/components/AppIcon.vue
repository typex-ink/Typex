<script setup lang="ts">
// App 图标（04 §2.2）：黑白随主题反转——浅色黑底白 glyph，深色白底黑 glyph
withDefaults(defineProps<{ size?: number }>(), { size: 88 });
</script>

<template>
  <div class="appicon" :style="{ width: size + 'px', height: size + 'px', borderRadius: size * 0.27 + 'px' }">
    <div class="bars" :style="{ gap: size * 0.058 + 'px', height: size * 0.385 + 'px' }">
      <i v-for="(h, idx) in [0.154, 0.269, 0.385, 0.269, 0.154]" :key="idx"
        :style="{ width: size * 0.077 + 'px', height: h * size + 'px' }" />
    </div>
    <div class="stem" :style="{
      width: size * 0.077 + 'px',
      height: size * 0.269 + 'px',
      marginTop: size * 0.058 + 'px',
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
}
</style>
