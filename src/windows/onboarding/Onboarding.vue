<script setup lang="ts">
// 首次启动引导 640×480，5 步（05 §6 / mockup §6）
import { onMounted, onUnmounted, ref } from "vue";
import Button from "@/components/Button.vue";
import AppIcon from "@/components/AppIcon.vue";
import Kbd from "@/components/Kbd.vue";
import SecretInput from "@/components/SecretInput.vue";
import Input from "@/components/Input.vue";
import { commands, events, type PermissionStatus } from "@/ipc/bindings";
import { useSettingsStore } from "@/stores/settings";

const store = useSettingsStore();
const step = ref(1);
const lang = ref<"zh_cn" | "en">("zh_cn");

// ── 步骤 2：权限（实时轮询）──
const perms = ref<PermissionStatus[]>([]);
let pollTimer: ReturnType<typeof setInterval> | null = null;
const PERM_META: Record<string, { icon: string; label: string; why: string }> = {
  microphone: { icon: "🎙", label: "麦克风", why: "录下你说的话" },
  accessibility: { icon: "⌨", label: "辅助功能", why: "把文字输入到光标处、读取选中文本" },
  input_monitoring: { icon: "👂", label: "输入监听", why: "监听全局快捷键（按住说话）" },
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
        label: "语音转文字", base_url: sttUrl.value.trim().replace(/\/+$/, ""),
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
        kind: "chat_completions", label: "大语言模型",
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
      <p class="slogan">{{ lang === "zh_cn" ? "说，即所得。" : "Speak. It types." }}</p>
    </div>

    <!-- 步骤 2 · 权限 -->
    <div v-else-if="step === 2" class="body">
      <h5>{{ lang === "zh_cn" ? "需要几项系统权限" : "A few system permissions" }}</h5>
      <div v-for="p in perms" :key="p.kind" class="perm">
        <span class="ic">{{ PERM_META[p.kind]?.icon }}</span>
        <span class="pmeta">
          {{ PERM_META[p.kind]?.label }}<br />
          <small>{{ PERM_META[p.kind]?.why }}</small>
        </span>
        <span v-if="p.granted" class="granted">✓ 已授权</span>
        <Button v-else size="sm" @click="commands.openPermissionSettings(p.kind)">去授权</Button>
      </div>
      <div v-if="!perms.length" class="perm">
        <span class="ic">🎙</span>
        <span class="pmeta">麦克风<br /><small>首次录音时系统将弹出授权</small></span>
        <span class="granted">—</span>
      </div>
    </div>

    <!-- 步骤 3 · 模型：云端直填（STT + LLM 两组；LLM 三槽共用） -->
    <div v-else-if="step === 3" class="body">
      <h5>连接模型服务</h5>
      <div class="slot-h">语音转文字</div>
      <div class="frow"><span>API 端点</span><span class="w250"><Input v-model="sttUrl" mono placeholder="https://api.example.com/v1" /></span></div>
      <div class="frow"><span>模型名</span><span class="w250"><Input v-model="sttModel" mono placeholder="whisper-large-v3-turbo" /></span></div>
      <div class="frow"><span>API 密钥</span><span class="w250"><SecretInput v-model="sttKey" /></span></div>
      <div class="slot-h">大语言模型（整理 · 翻译 · 问答共用，可在设置中分开）</div>
      <div class="frow"><span>API 端点</span><span class="w250"><Input v-model="llmUrl" mono placeholder="https://api.example.com/v1" /></span></div>
      <div class="frow"><span>模型名</span><span class="w250"><Input v-model="llmModel" mono placeholder="deepseek-chat" /></span></div>
      <div class="frow"><span>API 密钥</span><span class="w250"><SecretInput v-model="llmKey" /></span></div>
      <p v-if="configError" class="cfg-err">{{ configError }}</p>
    </div>

    <!-- 步骤 4 · 快捷键 + 练习 -->
    <div v-else-if="step === 4" class="body">
      <h5>试试你的快捷键</h5>
      <div class="frow"><span>听写</span><Kbd>右 ⌘</Kbd></div>
      <div class="frow"><span>助手</span><Kbd>右 ⌥</Kbd></div>
      <div class="frow"><span>翻译</span><span><Kbd>右 ⌘</Kbd> + <Kbd>右 ⌥</Kbd></span></div>
      <div class="practice">
        <p>练习：按住 <Kbd>右 ⌘</Kbd> 说「你好，Typex」</p>
        <Input v-model="practiceText" placeholder="文字会出现在这里…" @input="practiceDone = practiceText.length > 0" />
        <p v-if="practiceDone" class="aha">✓ 成功！这就是 Typex 的全部使用方式。</p>
      </div>
    </div>

    <!-- 步骤 5 · 完成 -->
    <div v-else class="body center">
      <span class="done-check">✓</span>
      <h5>一切就绪</h5>
      <label class="autostart-row">
        <input v-model="autostartOn" type="checkbox" />
        <span>{{ lang === "zh_cn" ? "登录时自动启动 Typex" : "Launch Typex at login" }}</span>
      </label>
    </div>

    <!-- 底部 -->
    <div class="foot">
      <template v-if="step === 1">
        <select v-model="lang" class="lang-select">
          <option value="zh_cn">简体中文</option>
          <option value="en">English</option>
        </select>
        <Button variant="primary" @click="step = 2">{{ lang === "zh_cn" ? "开始 →" : "Start →" }}</Button>
      </template>
      <template v-else-if="step === 5">
        <span />
        <Button variant="primary" @click="finish">完成</Button>
      </template>
      <template v-else>
        <Button variant="ghost" @click="step += 1">{{ step === 3 ? "稍后配置" : "跳过" }}</Button>
        <Button variant="primary" :disabled="configuring" @click="next">继续 →</Button>
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
