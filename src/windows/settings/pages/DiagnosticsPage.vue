<script setup lang="ts">
// 诊断页（mockup 2.12）
import { onMounted, ref } from "vue";
import Button from "@/components/Button.vue";
import { commands, type PermissionStatus } from "@/ipc/bindings";

const perms = ref<PermissionStatus[]>([]);

const LABEL: Record<string, string> = {
  microphone: "麦克风权限",
  accessibility: "辅助功能权限（注入 + 读选区）",
  input_monitoring: "输入监听权限（快捷键）",
};

onMounted(async () => {
  perms.value = await commands.getPermissionStatus();
});

function openSettings(kind: PermissionStatus["kind"]) {
  commands.openPermissionSettings(kind);
}
</script>

<template>
  <div>
    <h5 class="page-title">诊断</h5>
    <div v-for="p in perms" :key="p.kind" class="diag">
      <span :class="p.granted ? 'ok' : 'bad'">{{ p.granted ? "✓" : "✗" }}</span>
      <span>{{ LABEL[p.kind] ?? p.kind }}</span>
      <Button v-if="!p.granted" size="sm" class="fix" @click="openSettings(p.kind)">去授权</Button>
    </div>
    <div class="diag">
      <span class="ok">✓</span>
      <span>注入后端：剪贴板粘贴（CGEvent）</span>
    </div>
    <div class="actions">
      <Button>打开日志目录</Button>
      <Button>导出诊断包（自动脱敏）</Button>
    </div>
  </div>
</template>

<style scoped>
.page-title {
  font-size: 15px;
  margin-bottom: 14px;
  font-weight: 600;
}
.diag {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 0;
  border-bottom: 1px solid var(--border);
  font-size: 12.5px;
}
.ok {
  color: var(--success);
}
.bad {
  color: var(--error);
}
.fix {
  margin-left: auto;
}
.actions {
  display: flex;
  gap: 8px;
  margin-top: 14px;
}
</style>
