<script setup lang="ts">
// 历史设置页（mockup 2.11）
import FormRow from "@/components/FormRow.vue";
import Select from "@/components/Select.vue";
import Toggle from "@/components/Toggle.vue";
import Button from "@/components/Button.vue";
import { computed } from "vue";
import { useSetting } from "@/composables/useSetting";

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
    <h5 class="page-title">历史</h5>
    <p class="desc">历史仅保存在本机，不含音频。</p>
    <FormRow label="保存历史记录">
      <Toggle v-model="enabled" />
    </FormRow>
    <FormRow label="保留期限">
      <Select
        v-model="retention"
        :options="[
          { value: '7', label: '7 天' },
          { value: '30', label: '30 天' },
          { value: '90', label: '90 天' },
          { value: '0', label: '永久' },
        ]"
      />
    </FormRow>
    <FormRow label="清空全部历史">
      <Button variant="danger" size="sm">清空…</Button>
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
</style>
