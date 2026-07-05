<script setup lang="ts">
// ProviderCard 编辑子页（mockup 2.6）：预设下拉 → 按 kind 动态字段 → 保存/测试/删除
import { computed, ref } from "vue";
import Button from "@/components/Button.vue";
import FormRow from "@/components/FormRow.vue";
import Input from "@/components/Input.vue";
import SecretInput from "@/components/SecretInput.vue";
import Select from "@/components/Select.vue";
import { presetsForSlot } from "@/shared/presets";
import {
  commands,
  type ProviderProfile,
  type SlotKind,
} from "@/ipc/bindings";

const props = defineProps<{
  slotKind: SlotKind;
  profile: ProviderProfile | null;
}>();
const emit = defineEmits<{ back: []; saved: [] }>();

const SLOT_LABEL: Record<SlotKind, string> = {
  stt: "语音转文字",
  polish: "文本整理",
  translate: "翻译模型",
  assistant: "问答模型",
};

const presets = presetsForSlot(props.slotKind);
const presetId = ref<string>(presets[presets.length - 1].id); // 默认「自定义」
const label = ref(props.profile?.label ?? "");
const baseUrl = ref(props.profile?.base_url ?? "");
const model = ref(props.profile?.model ?? "");
const kind = ref(props.profile?.kind ?? (props.slotKind === "stt" ? "openai_compat" : "chat_completions"));
const apiKey = ref("");
// volcengine 双凭据（03 §2.2）
const appKey = ref("");
const accessToken = ref("");
const testResult = ref<string | null>(null);
const testOk = ref(false);
const saving = ref(false);

const isNew = computed(() => !props.profile);
const isVolc = computed(() => kind.value === "volcengine");
const hasExistingKey = computed(() => !!props.profile?.credentials?.["api_key"]);
const hasExistingVolcKeys = computed(
  () =>
    !!props.profile?.credentials?.["app_key"] &&
    !!props.profile?.credentials?.["access_token"],
);

function applyPreset(id: string) {
  presetId.value = id;
  const p = presets.find((x) => x.id === id);
  if (!p) return;
  if (p.base_url) baseUrl.value = p.base_url;
  kind.value = p.kind;
  if (p.models[0]) model.value = p.models[0];
  if (!label.value || presets.some((x) => x.label === label.value)) label.value = p.label;
}

const valid = computed(() => {
  if (!label.value.trim() || !model.value.trim()) return false;
  if (isVolc.value) {
    // 火山官方端点内置，base_url 留空即可
    return (
      (appKey.value.trim() && accessToken.value.trim()) || hasExistingVolcKeys.value
    );
  }
  return (
    baseUrl.value.trim().startsWith("http") &&
    (apiKey.value.trim() || hasExistingKey.value)
  );
});

async function save(): Promise<string | null> {
  if (!valid.value) return null;
  saving.value = true;
  const id = props.profile?.id ?? `p-${Date.now().toString(36)}`;
  const profile: ProviderProfile = {
    id,
    slots: props.profile?.slots ?? [props.slotKind],
    kind: kind.value,
    label: label.value.trim(),
    base_url: baseUrl.value.trim().replace(/\/+$/, ""),
    model: model.value.trim(),
    credentials: props.profile?.credentials ?? {},
    extra_headers: props.profile?.extra_headers ?? {},
    extra_form: props.profile?.extra_form ?? {},
    timeout_ms: props.profile?.timeout_ms ?? 30000,
    options: props.profile?.options ?? {},
  };
  const r = await commands.upsertProfile(profile);
  if (r.status !== "ok") {
    saving.value = false;
    return null;
  }
  if (isVolc.value) {
    if (appKey.value.trim()) {
      await commands.setProfileSecret(id, "app_key", appKey.value.trim());
    }
    if (accessToken.value.trim()) {
      await commands.setProfileSecret(id, "access_token", accessToken.value.trim());
    }
  } else if (apiKey.value.trim()) {
    await commands.setProfileSecret(id, "api_key", apiKey.value.trim());
  }
  if (isNew.value) {
    await commands.activateProfile(props.slotKind, id);
  }
  saving.value = false;
  return id;
}

async function saveAndBack() {
  const id = await save();
  if (id) emit("saved");
}

async function testConnection() {
  testResult.value = "测试中…";
  testOk.value = false;
  const id = await save();
  if (!id) {
    testResult.value = "请先完整填写表单";
    return;
  }
  const r = await commands.testProfile(id);
  if (r.status === "ok") {
    testResult.value = `✓ 测试通过 · ${r.data}ms`;
    testOk.value = true;
  } else {
    const map: Record<string, string> = {
      auth_error: "密钥无效（401）——请检查 API 密钥",
      network_error: "无法连接——请检查网络与端点地址",
      timeout: "响应超时——端点可能不可达",
      invalid_request: "请求被拒（404/400）——请检查 Base URL 与模型名",
      server_error: "服务端错误（5xx）——稍后再试",
    };
    testResult.value = `✗ ${map[r.error.code] ?? r.error.code}`;
    testOk.value = false;
  }
}

async function deleteProfile() {
  if (!props.profile) return;
  await commands.deleteProfile(props.profile.id);
  emit("saved");
}
</script>

<template>
  <div>
    <p class="back"><a @click="emit('back')">← 模型服务</a></p>
    <h5 class="page-title">{{ isNew ? "新建连接" : "编辑连接" }} — {{ SLOT_LABEL[slotKind] }}</h5>

    <FormRow label="预设">
      <Select
        :model-value="presetId"
        :options="presets.map((p) => ({ value: p.id, label: p.label }))"
        @update:model-value="applyPreset"
      />
    </FormRow>
    <FormRow label="名称">
      <span class="w280"><Input v-model="label" placeholder="如 Groq · whisper-turbo" /></span>
    </FormRow>
    <FormRow v-if="!isVolc" label="Base URL">
      <span class="w280"><Input v-model="baseUrl" mono placeholder="https://api.example.com/v1" /></span>
    </FormRow>
    <FormRow v-else label="端点" hint="留空使用官方端点 openspeech.bytedance.com">
      <span class="w280"><Input v-model="baseUrl" mono placeholder="（默认官方端点）" /></span>
    </FormRow>
    <FormRow label="模型">
      <span class="w280"><Input v-model="model" mono placeholder="模型名" /></span>
    </FormRow>
    <FormRow v-if="slotKind !== 'stt'" label="接口格式">
      <Select
        v-model="kind"
        :options="[
          { value: 'chat_completions', label: 'Chat Completions（OpenAI 兼容）' },
          { value: 'responses', label: 'Responses（OpenAI 新协议）' },
        ]"
      />
    </FormRow>
    <template v-if="isVolc">
      <FormRow label="APP ID" :hint="hasExistingVolcKeys ? '已保存在系统凭据库；留空则不修改' : '火山控制台的 APP ID'">
        <span class="w280"><SecretInput v-model="appKey" /></span>
      </FormRow>
      <FormRow label="Access Token" :hint="hasExistingVolcKeys ? undefined : '火山控制台的 Access Token'">
        <span class="w280"><SecretInput v-model="accessToken" /></span>
      </FormRow>
    </template>
    <FormRow v-else label="API 密钥" :hint="hasExistingKey ? '已保存在系统凭据库；留空则不修改' : undefined">
      <span class="w280"><SecretInput v-model="apiKey" /></span>
    </FormRow>

    <div class="actions">
      <Button variant="primary" :disabled="!valid || saving" @click="saveAndBack">保存</Button>
      <Button @click="testConnection">测试连接</Button>
      <span class="spacer" />
      <Button v-if="!isNew" variant="danger" @click="deleteProfile">删除档案</Button>
    </div>
    <p v-if="testResult" class="test-result" :class="{ ok: testOk }">{{ testResult }}</p>
  </div>
</template>

<style scoped>
.back {
  font-size: 12px;
  margin-bottom: 10px;
}
.back a {
  cursor: pointer;
}
.page-title {
  font-size: 15px;
  margin-bottom: 14px;
  font-weight: 600;
}
.w280 {
  width: 280px;
  display: inline-block;
}
.actions {
  display: flex;
  gap: 8px;
  margin-top: 16px;
  align-items: center;
}
.spacer {
  flex: 1;
}
.test-result {
  margin-top: 10px;
  font-size: 12px;
  color: var(--error);
}
.test-result.ok {
  color: var(--success);
}
</style>
