<script setup lang="ts">
// Callout（04 §7）：info/warn/error
defineProps<{ variant?: "info" | "warn" | "error"; icon?: string }>();
</script>

<template>
  <div class="callout" :class="variant ?? 'info'">
    <span class="ic">{{ icon ?? (variant === "error" ? "✕" : variant === "warn" ? "⚠" : "ⓘ") }}</span>
    <span><slot /></span>
  </div>
</template>

<style scoped>
.callout {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  border: 1px solid var(--border);
  border-left: 3px solid var(--text-2);
  background: var(--surface-2);
  border-radius: var(--radius-control);
  padding: 10px 12px;
  font-size: 12px;
  color: var(--text-2);
  line-height: 1.5;
}
.callout.error {
  border-left-color: var(--error);
  color: var(--error);
}
</style>
