<script setup lang="ts">
// 诊断页（05 §5.2）：环境自检 + 日志目录
import { onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Button from "@/components/Button.vue";
import { commands, type DiagnosticsReport, type PermissionStatus } from "@/ipc/bindings";

const { t } = useI18n();
const report = ref<DiagnosticsReport | null>(null);
const exporting = ref(false);
const exportResult = ref("");

async function exportPack() {
  exporting.value = true;
  exportResult.value = "";
  const r = await commands.exportDiagnostics();
  exporting.value = false;
  exportResult.value =
    r.status === "ok"
      ? t("settings.diagnostics.export_done", { path: r.data })
      : t("settings.diagnostics.export_failed");
}

const PERM_KEY: Record<string, string> = {
  microphone: "settings.diagnostics.perm_microphone",
  accessibility: "settings.diagnostics.perm_accessibility",
  input_monitoring: "settings.diagnostics.perm_input_monitoring",
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
    <h5 class="page-title">{{ t("settings.nav_diagnostics") }}</h5>
    <div class="diag">
      <span class="ok">✓</span>
      <span>{{ t("settings.diagnostics.platform", { platform: report.platform }) }}</span>
    </div>
    <div v-for="p in report.permissions" :key="p.kind" class="diag">
      <span :class="p.granted ? 'ok' : 'bad'">{{ p.granted ? "✓" : "✗" }}</span>
      <span>{{ PERM_KEY[p.kind] ? t(PERM_KEY[p.kind]) : p.kind }}</span>
      <Button v-if="!p.granted" size="sm" class="fix" @click="openSettings(p.kind)">
        {{ t("actions.grant_permission") }}
      </Button>
    </div>
    <div class="diag">
      <span class="ok">✓</span>
      <span>{{ t("settings.diagnostics.inject_backend", { backend: report.inject_backend }) }}</span>
    </div>
    <div class="actions">
      <Button @click="commands.openLogDir()">{{ t("settings.diagnostics.open_log_dir") }}</Button>
      <Button :disabled="exporting" @click="exportPack">
        {{ exporting ? t("settings.diagnostics.exporting") : t("settings.diagnostics.export") }}
      </Button>
    </div>
    <p v-if="exportResult" class="log-path">{{ exportResult }}</p>
    <p class="log-path">{{ t("settings.diagnostics.log_path", { path: report.log_dir }) }}</p>
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
