<script setup lang="ts">
// 首次启动引导 640×480，5 步（05 §6 / mockup §6）
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Button from "@/components/Button.vue";
import AppIcon from "@/components/AppIcon.vue";
import Kbd from "@/components/Kbd.vue";
import SecretInput from "@/components/SecretInput.vue";
import Input from "@/components/Input.vue";
import { commands, events, type HardwareTier, type LocalModelInfo, type PermissionStatus, type UiLanguage } from "@/ipc/bindings";
import { formatBytes } from "@/shared/format";
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

// ── 步骤 3：模型（本地一键推荐卡 + 云端直填两组表单，无厂商推荐——ADR-20/21）──
const sttUrl = ref("");
const sttKey = ref("");
const llmUrl = ref("");
const llmKey = ref("");
const sttModel = ref("");
const llmModel = ref("");
const configuring = ref(false);
const configError = ref("");

// 本地路径（CP-8.8 / mockup 步骤 3/3b）：档位检测 → 一键下载 → 三个功能指向 local 服务配置
const hw = ref<HardwareTier | null>(null);
const localModels = ref<LocalModelInfo[]>([]);
const chosenTier = ref<string>("standard");
const tierMenuOpen = ref(false);
// idle → downloading → done | error（下载在后台继续，不阻塞「下一步」）
const localPhase = ref<"idle" | "downloading" | "done" | "error">("idle");
const localError = ref("");
/** 当前正在下载的模型 id + 进度 */
const dlCurrent = ref<string | null>(null);
const dlDone = ref<number>(0);
const dlTotal = ref<number>(0);
let unlistenDl: (() => void) | null = null;

const TIER_KEYS = ["light", "standard", "performance"] as const;
/** 档位内两个模型（STT + LLM；按 tier 归属取，清单序 = STT 在前） */
const tierModels = computed(() =>
  localModels.value.filter((m) => m.tier === chosenTier.value),
);
const tierBytes = computed(() => tierModels.value.reduce((s, m) => s + m.bytes, 0));
const localAvailable = computed(() => hw.value !== null && localModels.value.length > 0);

function tierLabel(key: string): string {
  return t(`onboarding.tier_${key}`);
}

async function startLocalDownload() {
  const queue = tierModels.value.filter((m) => !m.downloaded);
  if (!queue.length) {
    await adoptLocalProfiles();
    localPhase.value = "done";
    return;
  }
  localPhase.value = "downloading";
  localError.value = "";
  await downloadNext(queue.map((m) => m.id));
}

/** 串行下载：完成一个再启动下一个；全部完成后落库本地服务配置 */
async function downloadNext(queue: string[]) {
  const [head, ...rest] = queue;
  if (!head) {
    await adoptLocalProfiles();
    localPhase.value = "done";
    dlCurrent.value = null;
    return;
  }
  dlCurrent.value = head;
  dlDone.value = 0;
  dlTotal.value = localModels.value.find((m) => m.id === head)?.bytes ?? 0;
  pendingQueue = rest;
  const r = await commands.downloadLocalModel(head, null);
  if (r.status !== "ok") {
    localPhase.value = "error";
    localError.value = r.error.message;
  }
}

let pendingQueue: string[] = [];

async function cancelLocalDownload() {
  if (dlCurrent.value) await commands.cancelLocalDownload(dlCurrent.value);
  pendingQueue = [];
  dlCurrent.value = null;
  localPhase.value = "idle";
}

/** 下载完成 → STT/整理/翻译功能指向 local 服务配置（ADR-20；问答槽不指向） */
async function adoptLocalProfiles() {
  const stt = tierModels.value.find((m) => m.purpose === "stt");
  const llm = tierModels.value.find((m) => m.purpose === "llm");
  if (stt) {
    await commands.upsertProfile({
      id: `local-${stt.id}`, capability: "stt", kind: "local",
      label: t("onboarding.local_profile_stt"), base_url: "",
      model: stt.id, credentials: {}, extra_headers: {}, extra_form: {},
      timeout_ms: 30000, options: {},
    });
    await commands.activateProfile("stt", `local-${stt.id}`);
  }
  if (llm) {
    await commands.upsertProfile({
      id: `local-${llm.id}`, capability: "llm", kind: "local",
      label: t("onboarding.local_profile_llm"), base_url: "",
      model: llm.id, credentials: {}, extra_headers: {}, extra_form: {},
      timeout_ms: 30000, options: {},
    });
    for (const slot of ["polish", "translate"] as const) {
      await commands.activateProfile(slot, `local-${llm.id}`);
    }
  }
}

async function loadLocal() {
  hw.value = await commands.getHardwareTier();
  if (hw.value) chosenTier.value = hw.value.tier;
  const r = await commands.listLocalModels();
  if (r.status === "ok") localModels.value = r.data;
}

async function saveModels(): Promise<boolean> {
  configError.value = "";
  const hasStt = sttUrl.value.trim() && sttKey.value.trim() && sttModel.value.trim();
  const hasLlm = llmUrl.value.trim() && llmKey.value.trim() && llmModel.value.trim();
  if (!hasStt && !hasLlm) return true; // 全空 = 稍后配置
  configuring.value = true;
  try {
    if (hasStt) {
      await commands.upsertProfile({
        id: "onboarding-stt", capability: "stt", kind: "openai_compat",
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
        id: "onboarding-llm", capability: "llm",
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
  await loadLocal();
  // 本地模型下载进度（串行队列：一个 done 就启动下一个；下载不阻塞走完余下步骤）
  unlistenDl = await events.localDownloadProgressEvent.listen((e) => {
    const p = e.payload;
    if (p.model_id !== dlCurrent.value) return;
    if (p.done) {
      if (p.error) {
        if (p.error !== "cancelled") {
          localPhase.value = "error";
          localError.value = p.error;
        }
        pendingQueue = [];
        dlCurrent.value = null;
      } else {
        const m = localModels.value.find((x) => x.id === p.model_id);
        if (m) m.downloaded = true;
        void downloadNext(pendingQueue);
      }
    } else if (p.bytes_total > 0) {
      dlDone.value = p.bytes_done;
      dlTotal.value = p.bytes_total;
    }
  });
});
onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
  unlistenDl?.();
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

    <!-- 步骤 3 · 模型：本地一键（推荐）或云端直填（STT + LLM 两组；三个 LLM 功能默认共用） -->
    <div v-else-if="step === 3" class="body">
      <h5>{{ t("onboarding.models_title") }}</h5>

      <!-- 本地模型推荐卡（CP-8.8 / mockup 步骤 3/3b；local-models 未启用时不显示） -->
      <div v-if="localAvailable && localPhase === 'idle'" class="prov on">
        <div class="plogo">◉</div>
        <div class="pmeta2">
          <b>{{ t("onboarding.local_title") }}</b>
          <span class="tag">{{ t("onboarding.local_recommended") }}</span><br />
          <small>
            {{ t("onboarding.local_detected", {
              tier: tierLabel(chosenTier),
              models: tierModels.map((m) => m.display_name).join(" + "),
              size: formatBytes(tierBytes),
            }) }}
            <span class="tier-wrap">
              <a @click="tierMenuOpen = !tierMenuOpen">{{ t("onboarding.local_change_tier") }}</a>
              <span v-if="tierMenuOpen" class="tier-menu">
                <a
                  v-for="k in TIER_KEYS"
                  :key="k"
                  :class="{ cur: k === chosenTier }"
                  @click="chosenTier = k; tierMenuOpen = false"
                >{{ (k === chosenTier ? "✓ " : "　") + tierLabel(k) }}</a>
              </span>
            </span>
          </small>
        </div>
        <Button size="sm" @click="startLocalDownload">{{ t("onboarding.local_download_use") }}</Button>
      </div>

      <!-- 下载中 / 完成 / 失败（mockup 步骤 3b：进度条 = --text-1 实底） -->
      <div v-else-if="localAvailable" class="prov on">
        <div class="plogo">◉</div>
        <div class="pmeta2">
          <b>{{ t("onboarding.local_title_tier", { tier: tierLabel(chosenTier) }) }}</b><br />
          <template v-if="localPhase === 'downloading'">
            <small>
              {{ t("onboarding.local_downloading", {
                name: localModels.find((m) => m.id === dlCurrent)?.display_name ?? "",
                done: formatBytes(dlDone),
                total: formatBytes(dlTotal),
              }) }}
            </small>
            <span class="pbar"><i :style="{ width: dlTotal ? `${Math.round((dlDone / dlTotal) * 100)}%` : '0%' }" /></span>
          </template>
          <small v-else-if="localPhase === 'done'" class="ok">{{ t("onboarding.local_done") }}</small>
          <small v-else class="err">{{ t("onboarding.local_failed", { err: localError }) }}</small>
        </div>
        <Button v-if="localPhase === 'downloading'" variant="ghost" size="sm" @click="cancelLocalDownload">
          {{ t("onboarding.local_cancel") }}
        </Button>
        <Button v-else-if="localPhase === 'error'" size="sm" @click="startLocalDownload">
          {{ t("onboarding.local_retry") }}
        </Button>
      </div>
      <p v-if="localAvailable && localPhase === 'downloading'" class="bg-hint">
        {{ t("onboarding.local_bg_hint") }}
      </p>

      <p v-if="localAvailable" class="divider">
        <span /><span class="dtext">{{ t("onboarding.or_cloud") }}</span><span />
      </p>

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
/* 本地推荐卡（mockup 步骤 3/3b） */
.prov {
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 11px 13px;
  font-size: 12.5px;
  margin-bottom: 8px;
  display: flex;
  align-items: center;
  gap: 10px;
}
.prov.on {
  background: var(--sel-bg);
  border-color: var(--border-2);
}
.plogo {
  width: 26px;
  height: 26px;
  border-radius: 6px;
  background: var(--surface-2);
  border: 1px solid var(--border);
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  color: var(--text-3);
}
.pmeta2 {
  flex: 1;
  line-height: 1.5;
  min-width: 0;
}
.pmeta2 small {
  color: var(--text-3);
  font-size: 11px;
}
.pmeta2 small.ok {
  color: var(--success);
}
.pmeta2 small.err {
  color: var(--error);
}
.pmeta2 a {
  text-decoration: underline;
  cursor: pointer;
}
.tag {
  font-size: 10px;
  border: 1px solid var(--border-2);
  border-radius: 4px;
  padding: 0 5px;
  margin-left: 4px;
  color: var(--text-2);
}
.tier-wrap {
  position: relative;
}
.tier-menu {
  position: absolute;
  left: 0;
  top: 16px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 8px;
  box-shadow: var(--shadow);
  padding: 4px;
  z-index: 10;
  display: flex;
  flex-direction: column;
  min-width: 90px;
}
.tier-menu a {
  padding: 5px 10px;
  border-radius: 5px;
  text-decoration: none;
  white-space: nowrap;
}
.tier-menu a:hover {
  background: var(--sel-bg);
}
.tier-menu a.cur {
  font-weight: 600;
}
/* 进度条 = --text-1 实底，无彩色（mockup 步骤 3b） */
.pbar {
  display: block;
  width: 100%;
  height: 4px;
  border-radius: 99px;
  background: var(--border);
  overflow: hidden;
  margin-top: 7px;
}
.pbar i {
  display: block;
  height: 100%;
  background: var(--text-1);
  transition: width 0.3s;
}
.bg-hint {
  font-size: 11px;
  color: var(--text-3);
  margin: 2px 0 8px;
}
.divider {
  font-size: 11px;
  color: var(--text-3);
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 10px 0;
}
.divider > span:not(.dtext) {
  flex: 1;
  border-top: 1px solid var(--border);
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
