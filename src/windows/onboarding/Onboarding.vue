<script setup lang="ts">
// 首次启动引导 640×480，5 步（05 §6 / mockup §6）
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Button from "@/components/Button.vue";
import AppIcon from "@/components/AppIcon.vue";
import Kbd from "@/components/Kbd.vue";
import SecretInput from "@/components/SecretInput.vue";
import Input from "@/components/Input.vue";
import { commands, events, type PermissionStatus, type UiLanguage } from "@/ipc/bindings";
import { useSettingsStore } from "@/stores/settings";

const { t } = useI18n();
const store = useSettingsStore();
const step = ref(1);
// 第 1 步语言下拉：直接写 settings.general.language，全 UI 即时切换（syncLocale 订阅）
const lang = computed<UiLanguage>({
  get: () => store.settings?.general.language ?? "system",
  set: (v) => void store.mutate((s) => (s.general.language = v)),
});

// ── 步骤 2：权限（实时轮询）──
const perms = ref<PermissionStatus[]>([]);
let pollTimer: ReturnType<typeof setInterval> | null = null;
const PERM_META: Record<string, { icon: string; labelKey: string; whyKey: string }> = {
  microphone: { icon: "🎙", labelKey: "onboarding.perm_microphone", whyKey: "onboarding.perm_microphone_why" },
  accessibility: { icon: "⌨", labelKey: "onboarding.perm_accessibility", whyKey: "onboarding.perm_accessibility_why" },
  input_monitoring: { icon: "👂", labelKey: "onboarding.perm_input_monitoring", whyKey: "onboarding.perm_input_monitoring_why" },
};

async function pollPerms() {
  perms.value = await commands.getPermissionStatus();
}

// ── 步骤 3：模型（云端直填两组表单，无厂商推荐——ADR-21）──
const sttUrl = ref("");
const sttKey = ref("");
const llmUrl = ref("");
const llmKey = ref("");
const sttModel = ref("");
const llmModel = ref("");
const configuring = ref(false);
const configError = ref("");

async function saveModels(): Promise<boolean> {
  configError.value = "";
  const hasStt = sttUrl.value.trim() && sttKey.value.trim() && sttModel.value.trim();
  const hasLlm = llmUrl.value.trim() && llmKey.value.trim() && llmModel.value.trim();
  if (!hasStt && !hasLlm) return true; // 全空 = 稍后配置
  configuring.value = true;
  try {
    if (hasStt) {
      await commands.upsertProfile({
        id: "onboarding-stt", slots: ["stt"], kind: "openai_compat",
        label: t("onboarding.stt_profile_label"), base_url: sttUrl.value.trim().replace(/\/+$/, ""),
        model: sttModel.value.trim(), credentials: {},
        extra_headers: {}, extra_form: {}, timeout_ms: 30000, options: {},
      });
      await commands.setProfileSecret("onboarding-stt", "api_key", sttKey.value.trim());
      await commands.activateProfile("stt", "onboarding-stt");
    }
    if (hasLlm) {
      // 三 LLM 槽共用同一连接（02 F-4）
      await commands.upsertProfile({
        id: "onboarding-llm", slots: ["polish", "translate", "assistant"],
        kind: "chat_completions", label: t("onboarding.llm_profile_label"),
        base_url: llmUrl.value.trim().replace(/\/+$/, ""),
        model: llmModel.value.trim(), credentials: {},
        extra_headers: {}, extra_form: {}, timeout_ms: 30000, options: {},
      });
      await commands.setProfileSecret("onboarding-llm", "api_key", llmKey.value.trim());
      for (const slot of ["polish", "translate", "assistant"] as const) {
        await commands.activateProfile(slot, "onboarding-llm");
      }
    }
    return true;
  } catch (e) {
    configError.value = String(e);
    return false;
  } finally {
    configuring.value = false;
  }
}

// ── 步骤 4：快捷键练习 ──
const practiceText = ref("");
const practiceDone = ref(false);

// ── 完成 ──
const autostartOn = ref(true); // 默认开启（02 F-6）
async function finish() {
  await store.mutate((s) => {
    s.onboarding_done = true;
    s.general.autostart = autostartOn.value;
  });
  window.close();
}

async function next() {
  if (step.value === 3 && !(await saveModels())) return;
  step.value += 1;
}

onMounted(async () => {
  await store.load();
  pollPerms();
  pollTimer = setInterval(pollPerms, 1500);
  await events.sessionSnapshotEvent.listen(() => {});
});
onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
});
</script>

<template>
  <div class="onb">
    <div class="titlebar" data-tauri-drag-region></div>
    <!-- 步骤圆点 -->
    <div class="steps">
      <i v-for="i in 5" :key="i" :class="{ on: step === i }" />
    </div>

    <!-- 步骤 1 · 欢迎：仅图标 + logotype + 口号 -->
    <div v-if="step === 1" class="body center">
      <AppIcon :size="88" />
      <div class="logotype">Typex</div>
      <p class="slogan">{{ t("onboarding.slogan") }}</p>
    </div>

    <!-- 步骤 2 · 权限 -->
    <div v-else-if="step === 2" class="body">
      <h5>{{ t("onboarding.perms_title") }}</h5>
      <div v-for="p in perms" :key="p.kind" class="perm">
        <span class="ic">{{ PERM_META[p.kind]?.icon }}</span>
        <span class="pmeta">
          {{ PERM_META[p.kind] ? t(PERM_META[p.kind].labelKey) : p.kind }}<br />
          <small>{{ PERM_META[p.kind] ? t(PERM_META[p.kind].whyKey) : "" }}</small>
        </span>
        <span v-if="p.granted" class="granted">{{ t("onboarding.granted") }}</span>
        <Button v-else size="sm" @click="commands.openPermissionSettings(p.kind)">
          {{ t("actions.grant_permission") }}
        </Button>
      </div>
      <div v-if="!perms.length" class="perm">
        <span class="ic">🎙</span>
        <span class="pmeta">{{ t("onboarding.perm_microphone") }}<br /><small>{{ t("onboarding.mic_pending") }}</small></span>
        <span class="granted">—</span>
      </div>
    </div>

    <!-- 步骤 3 · 模型：云端直填（STT + LLM 两组；LLM 三槽共用） -->
    <div v-else-if="step === 3" class="body">
      <h5>{{ t("onboarding.models_title") }}</h5>
      <div class="slot-h">{{ t("onboarding.slot_stt") }}</div>
      <div class="frow"><span>{{ t("onboarding.api_endpoint") }}</span><span class="w250"><Input v-model="sttUrl" mono placeholder="https://api.example.com/v1" /></span></div>
      <div class="frow"><span>{{ t("onboarding.model_name") }}</span><span class="w250"><Input v-model="sttModel" mono placeholder="whisper-large-v3-turbo" /></span></div>
      <div class="frow"><span>{{ t("onboarding.api_key") }}</span><span class="w250"><SecretInput v-model="sttKey" /></span></div>
      <div class="slot-h">{{ t("onboarding.slot_llm") }}</div>
      <div class="frow"><span>{{ t("onboarding.api_endpoint") }}</span><span class="w250"><Input v-model="llmUrl" mono placeholder="https://api.example.com/v1" /></span></div>
      <div class="frow"><span>{{ t("onboarding.model_name") }}</span><span class="w250"><Input v-model="llmModel" mono placeholder="deepseek-chat" /></span></div>
      <div class="frow"><span>{{ t("onboarding.api_key") }}</span><span class="w250"><SecretInput v-model="llmKey" /></span></div>
      <p v-if="configError" class="cfg-err">{{ configError }}</p>
    </div>

    <!-- 步骤 4 · 快捷键 + 练习 -->
    <div v-else-if="step === 4" class="body">
      <h5>{{ t("onboarding.hotkeys_title") }}</h5>
      <div class="frow"><span>{{ t("modes.dictation") }}</span><Kbd>{{ t("keys.MetaRight") }}</Kbd></div>
      <div class="frow"><span>{{ t("modes.assistant") }}</span><Kbd>{{ t("keys.AltGr") }}</Kbd></div>
      <div class="frow"><span>{{ t("modes.translation") }}</span><span><Kbd>{{ t("keys.MetaRight") }}</Kbd> + <Kbd>{{ t("keys.AltGr") }}</Kbd></span></div>
      <div class="practice">
        <p>
          <i18n-t keypath="onboarding.practice" scope="global">
            <template #key><Kbd>{{ t("keys.MetaRight") }}</Kbd></template>
          </i18n-t>
        </p>
        <Input v-model="practiceText" :placeholder="t('onboarding.practice_ph')" @input="practiceDone = practiceText.length > 0" />
        <p v-if="practiceDone" class="aha">{{ t("onboarding.practice_done") }}</p>
      </div>
    </div>

    <!-- 步骤 5 · 完成 -->
    <div v-else class="body center">
      <span class="done-check">✓</span>
      <h5>{{ t("onboarding.done_title") }}</h5>
      <label class="autostart-row">
        <input v-model="autostartOn" type="checkbox" />
        <span>{{ t("onboarding.autostart") }}</span>
      </label>
    </div>

    <!-- 底部 -->
    <div class="foot">
      <template v-if="step === 1">
        <select v-model="lang" class="lang-select">
          <option value="system">{{ t("onboarding.lang_system") }}</option>
          <option value="zh_cn">简体中文</option>
          <option value="en">English</option>
        </select>
        <Button variant="primary" @click="step = 2">{{ t("onboarding.start") }}</Button>
      </template>
      <template v-else-if="step === 5">
        <span />
        <Button variant="primary" @click="finish">{{ t("onboarding.finish") }}</Button>
      </template>
      <template v-else>
        <Button variant="ghost" @click="step += 1">{{ step === 3 ? t("onboarding.later") : t("onboarding.skip") }}</Button>
        <Button variant="primary" :disabled="configuring" @click="next">{{ t("onboarding.next") }}</Button>
      </template>
    </div>
  </div>
</template>

<style scoped>
.onb {
  width: 100vw;
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--surface);
  overflow: hidden;
}
.titlebar {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  height: 28px;
  z-index: 100;
}
.steps {
  display: flex;
  gap: 6px;
  justify-content: center;
  padding: 30px 0 0; /* 顶部让位红绿灯 */
  flex-shrink: 0;
}
.steps i {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--border-2);
  display: block;
}
.steps i.on {
  background: var(--primary);
}
.body {
  flex: 1;
  padding: 24px 32px;
  overflow-y: auto;
}
.body.center {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
  gap: 12px;
}
.body h5 {
  font-size: 16px;
  margin-bottom: 14px;
  font-weight: 600;
}
.logotype {
  font-size: 26px;
  font-weight: 600;
  letter-spacing: -0.01em;
}
.slogan {
  font-size: 14px;
}
.perm {
  display: flex;
  align-items: center;
  gap: 12px;
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 12px 14px;
  margin-bottom: 8px;
  font-size: 12.5px;
}
.perm .ic {
  font-size: 16px;
}
.pmeta {
  flex: 1;
  line-height: 1.5;
}
.pmeta small {
  color: var(--text-3);
}
.granted {
  color: var(--success);
  font-size: 12px;
}
.slot-h {
  font-size: 11px;
  color: var(--text-3);
  letter-spacing: 0.06em;
  margin: 12px 0 4px;
}
.frow {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 7px 0;
  border-bottom: 1px solid var(--border);
  font-size: 12.5px;
  gap: 16px;
}
.w250 {
  width: 250px;
  display: inline-block;
}
.practice {
  margin-top: 16px;
}
.practice p {
  font-size: 12px;
  color: var(--text-2);
  margin-bottom: 8px;
}
.aha {
  color: var(--success) !important;
  margin-top: 8px;
}
.cfg-err {
  color: var(--error);
  font-size: 11px;
  margin-top: 8px;
}
.done-check {
  font-size: 28px;
  color: var(--success);
}
.autostart-row {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 14px;
  font-size: 12px;
  color: var(--text-2);
  cursor: pointer;
}
.foot {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 14px 32px 18px;
  flex-shrink: 0;
}
.lang-select {
  min-width: 130px;
  height: 30px;
  font-size: 12.5px;
  border-radius: var(--radius-control);
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text-1);
  padding: 0 8px;
  font-family: inherit;
}
</style>
