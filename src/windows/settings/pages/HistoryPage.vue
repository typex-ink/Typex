<script setup lang="ts">
// 历史设置页（05 §5.2）
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
// 打字基准（05 §8：统计卡「节省时间」折算，默认 45 字/分）
const typingWpm = useSetting(
  (s) => s.history.typing_wpm,
  (s, v) => (s.history.typing_wpm = v),
);
</script>

<template>
  <div>
    <h5 class="page-title">{{ t("settings.nav_history") }}</h5>
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
    <FormRow :label="t('settings.history.typing_wpm')" :hint="t('settings.history.typing_wpm_hint')">
      <input
        v-model.number="typingWpm"
        type="range"
        min="15"
        max="120"
        step="5"
        class="slider"
      />
      <span class="mono wpm-val">{{ typingWpm }}</span>
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
.ok {
  color: var(--success);
  font-size: 12px;
}
.slider {
  width: 120px;
  accent-color: var(--primary);
}
.mono {
  font-family: var(--font-mono);
}
.wpm-val {
  font-size: 11px;
  color: var(--text-3);
}
</style>
