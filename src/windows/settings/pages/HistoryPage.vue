<script setup lang="ts">
// 历史设置页（mockup 2.11）
import FormRow from "@/components/FormRow.vue";
import Select from "@/components/Select.vue";
import Toggle from "@/components/Toggle.vue";
import Button from "@/components/Button.vue";
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { commands } from "@/ipc/bindings";
import { useSetting } from "@/composables/useSetting";

const { t } = useI18n();

const cleared = ref(false);
async function clearAll() {
  await commands.clearHistory();
  cleared.value = true;
  setTimeout(() => (cleared.value = false), 2000);
}

const enabled = useSetting(
  (s) => s.history.enabled,
  (s, v) => (s.history.enabled = v),
);
const retentionRaw = useSetting(
  (s) => s.history.retention_days,
  (s, v) => (s.history.retention_days = v),
);
const retention = computed({
  get: () => String(retentionRaw.value),
  set: (v) => (retentionRaw.value = Number(v)),
});
</script>

<template>
  <div>
    <h5 class="page-title">{{ t("settings.nav_history") }}</h5>
    <p class="desc">{{ t("settings.history.desc") }}</p>
    <FormRow :label="t('settings.history.save_toggle')">
      <Toggle v-model="enabled" />
    </FormRow>
    <FormRow :label="t('settings.history.retention')">
      <Select
        v-model="retention"
        :options="[
          { value: '7', label: t('settings.history.days', { n: 7 }) },
          { value: '30', label: t('settings.history.days', { n: 30 }) },
          { value: '90', label: t('settings.history.days', { n: 90 }) },
          { value: '0', label: t('settings.history.forever') },
        ]"
      />
    </FormRow>
    <FormRow :label="t('settings.history.clear')">
      <span v-if="cleared" class="ok">{{ t("settings.history.cleared") }}</span>
      <Button v-else variant="danger" size="sm" @click="clearAll">{{ t("settings.history.clear_btn") }}</Button>
    </FormRow>
  </div>
</template>

<style scoped>
.page-title {
  font-size: 15px;
  margin-bottom: 14px;
  font-weight: 600;
}
.desc {
  font-size: 12px;
  color: var(--text-2);
  margin: -10px 0 16px;
}
.ok {
  color: var(--success);
  font-size: 12px;
}
</style>
