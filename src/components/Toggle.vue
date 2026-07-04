<script setup lang="ts">
const model = defineModel<boolean>({ required: true });
defineProps<{ disabled?: boolean }>();
</script>

<template>
  <button
    class="toggle"
    :class="{ on: model }"
    role="switch"
    :aria-checked="model"
    :disabled="disabled"
    @click="model = !model"
  />
</template>

<style scoped>
.toggle {
  display: inline-block;
  width: 36px;
  height: 20px;
  border-radius: 99px;
  background: var(--border-2);
  position: relative;
  border: none;
  cursor: pointer;
  transition: background 0.12s ease-out;
  flex-shrink: 0;
  padding: 0;
}
.toggle::after {
  content: "";
  position: absolute;
  top: 2px;
  left: 2px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--surface);
  transition: left 0.12s ease-out;
}
.toggle.on {
  background: var(--primary);
}
.toggle.on::after {
  left: 18px;
  background: var(--on-primary);
}
.toggle:disabled {
  opacity: 0.45;
  cursor: default;
}
</style>
