<script setup lang="ts">
// SecretInput（04 §7）：密钥专用——默认掩码、显隐切换、粘贴按钮
import { ref } from "vue";
import { useI18n } from "vue-i18n";

const { t } = useI18n();
const model = defineModel<string>({ required: true });
defineProps<{ placeholder?: string }>();
const visible = ref(false);

async function paste() {
  try {
    model.value = await navigator.clipboard.readText();
  } catch {
    /* 剪贴板权限拒绝时静默 */
  }
}
</script>

<template>
  <span class="secret">
    <input
      v-model="model"
      :type="visible ? 'text' : 'password'"
      :placeholder="placeholder ?? 'sk-…'"
      spellcheck="false"
      autocomplete="off"
    />
    <button type="button" :title="visible ? t('components.secret.hide') : t('components.secret.show')" @click="visible = !visible">
      {{ visible ? "🙈" : "👁" }}
    </button>
    <button type="button" :title="t('components.secret.paste')" @click="paste">📋</button>
  </span>
</template>

<style scoped>
.secret {
  display: flex;
  align-items: center;
  border: 1px solid var(--border);
  background: var(--surface-2);
  border-radius: var(--radius-control);
  height: 32px;
  padding-right: 4px;
  width: 100%;
}
.secret input {
  flex: 1;
  border: none;
  background: transparent;
  color: var(--text-1);
  padding: 0 10px;
  font-family: var(--font-mono);
  font-size: 12px;
  outline: none;
  min-width: 0;
}
.secret input::placeholder {
  color: var(--text-3);
}
.secret button {
  border: none;
  background: transparent;
  color: var(--text-3);
  font-size: 12px;
  padding: 4px 6px;
  cursor: pointer;
}
.secret button:hover {
  color: var(--text-1);
}
</style>
