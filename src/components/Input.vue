<script setup lang="ts">
const model = defineModel<string | number>({ required: true });
withDefaults(
  defineProps<{
    placeholder?: string;
    disabled?: boolean;
    mono?: boolean;
    type?: "text" | "number";
    min?: number;
    max?: number;
    step?: number;
  }>(),
  { type: "text" },
);
</script>

<template>
  <input
    v-model="model"
    class="input"
    :class="{ mono }"
    :type="type"
    :min="min"
    :max="max"
    :step="step"
    :placeholder="placeholder"
    :disabled="disabled"
    spellcheck="false"
  />
</template>

<style scoped>
.input {
  height: 32px;
  border-radius: var(--radius-control);
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text-1);
  padding: 0 10px;
  font-size: 13px;
  font-family: inherit;
  width: 100%;
}
.input.mono {
  font-family: var(--font-mono);
  font-size: 12px;
}
.input::placeholder {
  color: var(--text-3);
}
.input:disabled {
  opacity: 0.45;
}
</style>
