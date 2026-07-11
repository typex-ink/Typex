<script setup lang="ts">
// 通用页（05 §5.2）
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import FormRow from "@/components/FormRow.vue";
import Select from "@/components/Select.vue";
import Toggle from "@/components/Toggle.vue";
import { useSetting } from "@/composables/useSetting";

const { t } = useI18n();

const language = useSetting(
  (s) => s.general.language,
  (s, v) => (s.general.language = v),
);
const theme = useSetting(
  (s) => s.general.theme,
  (s, v) => (s.general.theme = v),
);
const autostart = useSetting(
  (s) => s.general.autostart,
  (s, v) => (s.general.autostart = v),
);
const chimesEnabled = useSetting(
  (s) => s.general.chimes_enabled,
  (s, v) => (s.general.chimes_enabled = v),
);
const chimesVolume = useSetting(
  (s) => s.general.chimes_volume,
  (s, v) => (s.general.chimes_volume = v),
);
const chimesVolumePercent = computed({
  get: () => Math.round(chimesVolume.value * 100),
  set: (value: number) => {
    chimesVolume.value = Math.min(100, Math.max(0, value)) / 100;
  },
});
const proxyMode = useSetting(
  (s) => s.general.proxy_mode,
  (s, v) => (s.general.proxy_mode = v),
);
const updateChannel = useSetting(
  (s) => s.general.update_channel,
  (s, v) => (s.general.update_channel = v),
);
</script>

<template>
  <div>
    <h5 class="page-title">{{ t("settings.nav_general") }}</h5>
    <FormRow :label="t('settings.general.language')">
      <Select
        v-model="language"
        :options="[
          { value: 'system', label: t('settings.general.lang_system') },
          { value: 'zh_cn', label: '简体中文' },
          { value: 'en', label: 'English' },
        ]"
      />
    </FormRow>
    <FormRow :label="t('settings.general.theme')">
      <Select
        v-model="theme"
        :options="[
          { value: 'system', label: t('settings.general.theme_system') },
          { value: 'light', label: t('settings.general.theme_light') },
          { value: 'dark', label: t('settings.general.theme_dark') },
        ]"
      />
    </FormRow>
    <FormRow :label="t('settings.general.autostart')">
      <Toggle v-model="autostart" />
    </FormRow>
    <FormRow :label="t('settings.general.chimes')" :hint="t('settings.general.chimes_hint')">
      <Toggle v-model="chimesEnabled" />
    </FormRow>
    <FormRow :label="t('settings.general.chimes_volume')">
      <input
        v-model.number="chimesVolumePercent"
        type="range"
        min="0"
        max="100"
        step="5"
        class="slider"
        :disabled="!chimesEnabled"
        :aria-label="t('settings.general.chimes_volume')"
      />
      <span class="mono volume-val">{{ chimesVolumePercent }}%</span>
    </FormRow>
    <FormRow :label="t('settings.general.proxy')">
      <Select
        v-model="proxyMode"
        :options="[
          { value: 'system', label: t('settings.general.proxy_system') },
          { value: 'manual', label: t('settings.general.proxy_manual') },
          { value: 'direct', label: t('settings.general.proxy_direct') },
        ]"
      />
    </FormRow>
    <FormRow :label="t('settings.general.update_channel')">
      <Select
        v-model="updateChannel"
        :options="[
          { value: 'stable', label: t('settings.general.channel_stable') },
          { value: 'nightly', label: t('settings.general.channel_nightly') },
        ]"
      />
    </FormRow>
  </div>
</template>

<style scoped>
.page-title {
  font-size: 15px;
  margin-bottom: 14px;
  font-weight: 600;
}
.slider {
  width: 120px;
  accent-color: var(--primary);
}
.slider:disabled {
  cursor: default;
  opacity: 0.45;
}
.mono {
  font-family: var(--font-mono);
}
.volume-val {
  width: 34px;
  color: var(--text-3);
  font-size: 11px;
  text-align: right;
}
</style>
