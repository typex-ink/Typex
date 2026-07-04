<script setup lang="ts">
// Kbd：快捷键展示（mockup .kbd）
</script>

<template>
  <kbd class="kbd"><slot /></kbd>
</template>

<style scoped>
.kbd {
  font-family: var(--font-mono);
  font-size: 11px;
  border: 1px solid var(--border);
  border-bottom-width: 2px;
  border-radius: 5px;
  padding: 2px 7px;
  background: var(--surface-2);
  white-space: nowrap;
}
</style>
