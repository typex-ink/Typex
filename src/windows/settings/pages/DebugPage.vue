<script setup lang="ts">
// 调试页：开发/测试入口，不改变真实业务配置。
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import Button from "@/components/Button.vue";
import { commands } from "@/ipc/bindings";

const { t } = useI18n();
const opening = ref(false);
const status = ref("");

async function openOnboarding() {
  opening.value = true;
  status.value = "";
  const result = await commands.openOnboardingWindow();
  opening.value = false;
  status.value =
    result.status === "ok"
      ? t("settings.debug.onboarding_opened")
      : t("settings.debug.onboarding_failed");
}
</script>

<template>
  <div>
    <h5 class="page-title">{{ t("settings.nav_debug") }}</h5>
    <div class="row">
      <div>
        <p class="label">{{ t("settings.debug.onboarding") }}</p>
        <p class="hint">{{ t("settings.debug.onboarding_hint") }}</p>
      </div>
      <Button :disabled="opening" @click="openOnboarding">
        {{ opening ? t("settings.debug.opening") : t("settings.debug.reopen_onboarding") }}
      </Button>
    </div>
    <p v-if="status" class="status">{{ status }}</p>
  </div>
</template>

<style scoped>
.page-title {
  font-size: 15px;
  margin-bottom: 14px;
  font-weight: 600;
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 12px 0;
  border-bottom: 1px solid var(--border);
}
.label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.hint {
  margin-top: 4px;
  font-size: 12px;
  color: var(--text-2);
}
.status {
  margin-top: 10px;
  font-size: 12px;
  color: var(--text-2);
}
</style>
