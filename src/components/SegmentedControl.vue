<script setup lang="ts">
import { nextTick, ref } from "vue";

type Option = { value: string; label: string };

const props = defineProps<{
  modelValue: string;
  options: Option[];
  groupLabel: string;
}>();
const emit = defineEmits<{ "update:modelValue": [value: string] }>();
const buttons = ref<HTMLButtonElement[]>([]);

function select(index: number) {
  const option = props.options[index];
  if (!option) return;
  emit("update:modelValue", option.value);
  void nextTick(() => buttons.value[index]?.focus());
}

function onKeydown(event: KeyboardEvent) {
  const current = Math.max(
    0,
    props.options.findIndex((option) => option.value === props.modelValue),
  );
  let next: number | null = null;
  if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
    next = (current - 1 + props.options.length) % props.options.length;
  } else if (event.key === "ArrowRight" || event.key === "ArrowDown") {
    next = (current + 1) % props.options.length;
  } else if (event.key === "Home") {
    next = 0;
  } else if (event.key === "End") {
    next = props.options.length - 1;
  }
  if (next === null) return;
  event.preventDefault();
  select(next);
}
</script>

<template>
  <div class="segmented" role="radiogroup" :aria-label="groupLabel" @keydown="onKeydown">
    <button
      v-for="(option, index) in options"
      :key="option.value"
      :ref="(element) => element && (buttons[index] = element as HTMLButtonElement)"
      type="button"
      role="radio"
      :aria-checked="option.value === modelValue"
      :tabindex="option.value === modelValue ? 0 : -1"
      :class="{ selected: option.value === modelValue }"
      @click="select(index)"
    >
      {{ option.label }}
    </button>
  </div>
</template>

<style scoped>
.segmented {
  display: inline-grid;
  grid-auto-flow: column;
  grid-auto-columns: minmax(92px, 1fr);
  gap: 2px;
  padding: 2px;
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  background: var(--surface-2);
}
.segmented button {
  min-height: 28px;
  border: 0;
  border-radius: 6px;
  padding: 4px 10px;
  background: transparent;
  color: var(--text-2);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}
.segmented button.selected {
  background: var(--surface);
  color: var(--text-1);
  font-weight: 600;
  box-shadow: 0 1px 2px var(--border);
}
.segmented button:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: 1px;
}
</style>
