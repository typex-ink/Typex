<script setup lang="ts">
// 关于页（mockup 2.13）：图标 + logotype + 版本 + 检查更新（CP-6.3 / ADR-11）
import { onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Button from "@/components/Button.vue";
import AppIcon from "@/components/AppIcon.vue";
import { commands, events } from "@/ipc/bindings";

const { t } = useI18n();
const checking = ref(false);
const installing = ref(false);
const status = ref("");
const available = ref<{ version: string; notes: string } | null>(null);

async function check() {
  checking.value = true;
  status.value = "";
  const r = await commands.checkUpdate();
  checking.value = false;
  if (r.status !== "ok") {
    status.value = t("settings.about.check_failed");
    return;
  }
  if (r.data) {
    available.value = r.data;
  } else {
    status.value = t("settings.about.up_to_date");
  }
}

async function install() {
  installing.value = true;
  const r = await commands.installUpdate();
  // 成功时应用会重启；走到这里说明失败
  if (r.status !== "ok") {
    installing.value = false;
    status.value = t("settings.about.install_failed");
  }
}

const unlisteners: (() => void)[] = [];
onMounted(async () => {
  // 启动自动检查/托盘检查发现的新版本（ADR-11：安装需确认）
  unlisteners.push(
    await events.updateAvailableEvent.listen((e) => {
      available.value = e.payload;
    }),
  );
});
onUnmounted(() => unlisteners.forEach((u) => u()));
</script>

<template>
  <div class="about">
    <AppIcon :size="88" />
    <div class="logotype">Typex</div>
    <p class="meta">
      v0.1.1 · GPL-3.0 · typex.ink<br />
      {{ t("settings.about.privacy") }}
    </p>
    <div v-if="available" class="update-card">
      <p class="update-title">{{ t("settings.about.new_version", { version: available.version }) }}</p>
      <p v-if="available.notes" class="update-notes">{{ available.notes }}</p>
      <Button variant="primary" size="sm" :disabled="installing" @click="install">
        {{ installing ? t("settings.about.installing") : t("settings.about.install") }}
      </Button>
    </div>
    <div class="actions">
      <Button size="sm" :disabled="checking" @click="check">
        {{ checking ? t("settings.about.checking") : t("settings.about.check") }}
      </Button>
      <Button variant="ghost" size="sm">{{ t("settings.about.licenses") }}</Button>
    </div>
    <p v-if="status" class="status">{{ status }}</p>
  </div>
</template>

<style scoped>
.about {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
  gap: 10px;
  height: 100%;
  min-height: 380px;
}
/* logotype：Inter SemiBold 字距 -1%，五字母同色（04 §2.3） */
.logotype {
  font-size: 26px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--text-1);
}
.meta {
  font-size: 12px;
  color: var(--text-2);
  line-height: 1.6;
}
.actions {
  display: flex;
  gap: 8px;
}
.update-card {
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  padding: 12px 16px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  max-width: 320px;
}
.update-title {
  font-size: 13px;
  font-weight: 600;
}
.update-notes {
  font-size: 11px;
  color: var(--text-2);
  max-height: 80px;
  overflow-y: auto;
  white-space: pre-wrap;
}
.status {
  font-size: 12px;
  color: var(--text-2);
}
</style>
