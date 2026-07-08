<script setup lang="ts">
// 设置表单行：左标签（可带说明小字）+ 右控件
defineProps<{ label: string; hint?: string }>();
</script>

<template>
  <div class="frow">
    <span class="l">
      {{ label }}
      <small v-if="hint">{{ hint }}</small>
    </span>
    <span class="r"><slot /></span>
  </div>
</template>

<style scoped>
.frow {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 9px 0;
  border-bottom: 1px solid var(--border);
  font-size: 12.5px;
  gap: 16px;
}
.frow:last-child {
  border-bottom: none;
}
.l {
  color: var(--text-1);
  min-width: 0;
  flex: 1;
  overflow-wrap: anywhere;
}
.l small {
  display: block;
  color: var(--text-3);
  font-size: 11px;
  margin-top: 2px;
}
.r {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
</style>
