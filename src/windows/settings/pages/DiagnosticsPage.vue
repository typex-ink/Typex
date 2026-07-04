<script setup lang="ts">
// 诊断页（mockup 2.12）：环境自检 + 日志目录
import { onMounted, ref } from "vue";
import Button from "@/components/Button.vue";
import { commands, type DiagnosticsReport, type PermissionStatus } from "@/ipc/bindings";

const report = ref<DiagnosticsReport | null>(null);

const LABEL: Record<string, string> = {
  microphone: "麦克风权限",
  accessibility: "辅助功能权限（注入 + 读选区）",
  input_monitoring: "输入监听权限（快捷键）",
};

onMounted(async () => {
  report.value = await commands.getDiagnostics();
});

function openSettings(kind: PermissionStatus["kind"]) {
  commands.openPermissionSettings(kind);
}
</script>

<template>
  <div v-if="report">
    <h5 class="page-title">诊断</h5>
    <div class="diag">
      <span class="ok">✓</span>
      <span>平台：{{ report.platform }}</span>
    </div>
    <div v-for="p in report.permissions" :key="p.kind" class="diag">
      <span :class="p.granted ? 'ok' : 'bad'">{{ p.granted ? "✓" : "✗" }}</span>
      <span>{{ LABEL[p.kind] ?? p.kind }}</span>
      <Button v-if="!p.granted" size="sm" class="fix" @click="openSettings(p.kind)">去授权</Button>
    </div>
    <div class="diag">
      <span class="ok">✓</span>
      <span>注入后端：{{ report.inject_backend }}</span>
    </div>
    <div class="actions">
      <Button @click="commands.openLogDir()">打开日志目录</Button>
    </div>
    <p class="log-path">日志：{{ report.log_dir }}</p>
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
.log-path {
  font-size: 11px;
  color: var(--text-3);
  font-family: var(--font-mono);
  margin-top: 10px;
  user-select: text;
}
</style>
