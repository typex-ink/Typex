<script setup lang="ts">
// Button（04 §7）：primary/secondary/ghost/danger，高 32（sm 26），radius 8
defineProps<{
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "md" | "sm";
  disabled?: boolean;
}>();
</script>

<template>
  <button
    class="btn"
    :class="[`btn-${variant ?? 'secondary'}`, size === 'sm' ? 'btn-sm' : '']"
    :disabled="disabled"
  >
    <slot />
  </button>
</template>

<style scoped>
.btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 32px;
  padding: 0 14px;
  border-radius: var(--radius-control);
  font-size: 13px;
  font-weight: 500;
  border: none;
  cursor: pointer;
  font-family: inherit;
  transition: background 0.12s ease-out, color 0.12s ease-out;
  white-space: nowrap;
}
.btn-primary {
  background: var(--primary);
  color: var(--on-primary);
}
.btn-primary:hover:not(:disabled) {
  background: var(--primary-hover);
}
.btn-secondary {
  background: transparent;
  color: var(--text-1);
  border: 1px solid var(--border-2);
}
.btn-secondary:hover:not(:disabled) {
  background: var(--surface-2);
}
.btn-ghost {
  background: transparent;
  color: var(--text-2);
  text-decoration: underline;
  text-underline-offset: 3px;
}
.btn-ghost:hover:not(:disabled) {
  color: var(--text-1);
}
.btn-danger {
  background: transparent;
  color: var(--error);
  border: 1px solid currentColor;
}
.btn-sm {
  height: 26px;
  font-size: 12px;
  padding: 0 10px;
}
.btn:disabled {
  opacity: 0.45;
  cursor: default;
}
</style>
