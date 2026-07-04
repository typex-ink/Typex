<script setup lang="ts">
export interface Option {
  value: string;
  label: string;
}
const model = defineModel<string>({ required: true });
defineProps<{ options: Option[]; disabled?: boolean }>();
</script>

<template>
  <span class="select-wrap">
    <select v-model="model" class="select" :disabled="disabled">
      <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
    </select>
    <span class="arr">▾</span>
  </span>
</template>

<style scoped>
.select-wrap {
  position: relative;
  display: inline-flex;
  align-items: center;
  min-width: 150px;
}
.select {
  appearance: none;
  width: 100%;
  height: 32px;
  padding: 0 28px 0 10px;
  border-radius: var(--radius-control);
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text-1);
  font-size: 13px;
  font-family: inherit;
  cursor: pointer;
}
.arr {
  position: absolute;
  right: 10px;
  color: var(--text-3);
  font-size: 10px;
  pointer-events: none;
}
.select:disabled {
  opacity: 0.45;
}
</style>
