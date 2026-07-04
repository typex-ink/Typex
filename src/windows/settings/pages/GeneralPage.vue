<script setup lang="ts">
// 通用页（mockup 2.1）
import FormRow from "@/components/FormRow.vue";
import Select from "@/components/Select.vue";
import Toggle from "@/components/Toggle.vue";
import { useSetting } from "@/composables/useSetting";

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
const chimes = useSetting(
  (s) => s.general.chimes_enabled,
  (s, v) => (s.general.chimes_enabled = v),
);
const proxyMode = useSetting(
  (s) => s.general.proxy_mode,
  (s, v) => (s.general.proxy_mode = v),
);
</script>

<template>
  <div>
    <h5 class="page-title">通用</h5>
    <FormRow label="界面语言">
      <Select
        v-model="language"
        :options="[
          { value: 'system', label: '跟随系统' },
          { value: 'zh_cn', label: '简体中文' },
          { value: 'en', label: 'English' },
        ]"
      />
    </FormRow>
    <FormRow label="主题">
      <Select
        v-model="theme"
        :options="[
          { value: 'system', label: '跟随系统' },
          { value: 'light', label: '亮色' },
          { value: 'dark', label: '暗色' },
        ]"
      />
    </FormRow>
    <FormRow label="开机自启">
      <Toggle v-model="autostart" />
    </FormRow>
    <FormRow label="提示音" hint="开始 / 成功 / 错误 三枚，可关">
      <Toggle v-model="chimes" />
    </FormRow>
    <FormRow label="网络代理">
      <Select
        v-model="proxyMode"
        :options="[
          { value: 'system', label: '跟随系统' },
          { value: 'manual', label: '手动' },
          { value: 'direct', label: '直连' },
        ]"
      />
    </FormRow>
    <FormRow label="更新通道">
      <Select model-value="stable" :options="[{ value: 'stable', label: '稳定版' }]" />
    </FormRow>
  </div>
</template>

<style scoped>
.page-title {
  font-size: 15px;
  margin-bottom: 14px;
  font-weight: 600;
}
</style>
