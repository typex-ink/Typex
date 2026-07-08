<script setup lang="ts">
// ProviderCard 编辑子页（mockup 2.6 云端 / 2.7 本地）：预设下拉 → 按 kind 动态字段 → 保存/测试/删除
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Button from "@/components/Button.vue";
import FormRow from "@/components/FormRow.vue";
import Input from "@/components/Input.vue";
import SecretInput from "@/components/SecretInput.vue";
import Select from "@/components/Select.vue";
import { presetsForCapability } from "@/shared/presets";
import { formatBytes } from "@/shared/format";
import {
  commands,
  events,
  type LocalModelInfo,
  type ProviderCapability,
  type ProviderProfile,
  type SlotKind,
  type TypexError,
} from "@/ipc/bindings";

const props = defineProps<{
  capability: ProviderCapability;
  profile: ProviderProfile | null;
  assignTo?: SlotKind | null;
}>();
const emit = defineEmits<{ back: []; saved: [] }>();

const { t } = useI18n();

const CAPABILITY_LABEL_KEY: Record<ProviderCapability, string> = {
  stt: "settings.providers.service_stt",
  llm: "settings.providers.service_llm",
};
const ERROR_KEYS: Record<string, string> = {
  auth_error: "settings.profile.err_auth_error",
  network_error: "settings.profile.err_network_error",
  timeout: "settings.profile.err_timeout",
  invalid_request: "settings.profile.err_invalid_request",
  server_error: "settings.profile.err_server_error",
  not_configured: "settings.profile.err_not_configured",
};

const presets = presetsForCapability(props.capability);
const REASONING_EFFORTS = ["none", "minimal", "low", "medium", "high", "xhigh"] as const;
type ReasoningEffort = (typeof REASONING_EFFORTS)[number];

function isReasoningEffort(value: unknown): value is ReasoningEffort {
  return typeof value === "string" && (REASONING_EFFORTS as readonly string[]).includes(value);
}

function initialReasoningEffort(profile: ProviderProfile | null): ReasoningEffort {
  const raw = profile?.options?.["reasoning_effort"];
  if (isReasoningEffort(raw)) return raw;
  const legacy = profile?.options?.["enable_thinking"];
  if (legacy === true) return "medium";
  if (legacy === false) return "none";
  return "none";
}

const presetId = ref<string>(
  props.profile?.kind === "local"
    ? (presets.find((p) => p.kind === "local")?.id ?? presets[presets.length - 1].id)
    : presets[presets.length - 1].id, // 默认「自定义」
);
const label = ref(props.profile?.label ?? "");
const baseUrl = ref(props.profile?.base_url ?? "");
const model = ref(props.profile?.model ?? "");
const kind = ref(props.profile?.kind ?? (props.capability === "stt" ? "openai_compat" : "chat_completions"));
const apiKey = ref("");
// volcengine 双凭据（03 §2.2）
const appKey = ref("");
const accessToken = ref("");
const testResult = ref<string | null>(null);
const testOk = ref(false);
const saving = ref(false);

const isNew = computed(() => !props.profile);
const isVolc = computed(() => kind.value === "volcengine");
const isLocal = computed(() => kind.value === "local");
const canConfigureReasoning = computed(
  () =>
    props.capability !== "stt" &&
    (kind.value === "chat_completions" || kind.value === "responses" || kind.value === "local"),
);
const reasoningOptions = computed(() => [
  { value: "none", label: t("settings.profile.reasoning_none") },
  { value: "minimal", label: t("settings.profile.reasoning_minimal") },
  { value: "low", label: t("settings.profile.reasoning_low") },
  { value: "medium", label: t("settings.profile.reasoning_medium") },
  { value: "high", label: t("settings.profile.reasoning_high") },
  { value: "xhigh", label: t("settings.profile.reasoning_xhigh") },
]);
function hasStoredSecret(value: string | undefined): boolean {
  return !!value?.trim() && !value.trim().startsWith("keyring://");
}

const hasExistingKey = computed(() => hasStoredSecret(props.profile?.credentials?.["api_key"]));
const hasExistingVolcKeys = computed(
  () =>
    hasStoredSecret(props.profile?.credentials?.["app_key"]) &&
    hasStoredSecret(props.profile?.credentials?.["access_token"]),
);

// ── 本地档案编辑态（CP-8.7 / mockup 2.7）──
// 模型下拉来自模型库，按槽位 purpose 过滤：stt 槽列 stt 模型，其余槽列 llm。
const localModels = ref<LocalModelInfo[]>([]);
const loadPolicy = ref<string>(
  (props.profile?.options?.["load_policy"] as string) ?? "resident",
);
const reasoningEffort = ref(initialReasoningEffort(props.profile));
const downloadPct = ref<number | null>(null);
let unlistenProgress: (() => void) | null = null;

const slotPurpose = props.capability;
const localOptions = computed(() =>
  localModels.value
    .filter((m) => m.purpose === slotPurpose)
    .map((m) => ({
      value: m.id,
      label: `${m.display_name}${m.origin === "imported" ? ` · ${t("settings.models.origin_imported")}` : ""}（${
        m.downloaded
          ? t("settings.profile.local_downloaded", { size: formatBytes(m.bytes) })
          : t("settings.profile.local_not_downloaded")
      }）`,
    })),
);
const selectedLocal = computed(
  () => localModels.value.find((m) => m.id === model.value) ?? null,
);

async function loadLocalModels() {
  const r = await commands.listLocalModels();
  if (r.status === "ok") {
    localModels.value = r.data;
    if (isLocal.value && !model.value) {
      model.value = localModels.value.find((m) => m.purpose === slotPurpose)?.id ?? "";
    }
  }
}

async function downloadSelected() {
  if (!selectedLocal.value) return;
  downloadPct.value = 0;
  await commands.downloadLocalModel(selectedLocal.value.id, null);
}

function profileErrorMessage(error: TypexError): string {
  const key = ERROR_KEYS[error.code];
  const upstream = error.message?.trim();
  return `✗ ${key ? t(key) : error.code}${upstream ? `：${upstream}` : ""}`;
}

async function saveSecret(id: string, field: string, value: string): Promise<boolean> {
  const result = await commands.setProfileSecret(id, field, value);
  if (result.status === "ok") return true;
  testResult.value = profileErrorMessage(result.error);
  testOk.value = false;
  return false;
}

function displayLabel(p: (typeof presets)[number]): string {
  return p.labelKey ? t(p.labelKey) : p.label;
}

function applyPreset(id: string) {
  presetId.value = id;
  const p = presets.find((x) => x.id === id);
  if (!p) return;
  if (p.base_url) baseUrl.value = p.base_url;
  kind.value = p.kind;
  if (p.kind === "local") {
    baseUrl.value = "";
    model.value = localModels.value.find((m) => m.purpose === slotPurpose)?.id ?? "";
    void loadLocalModels();
  } else if (p.models[0]) {
    model.value = p.models[0];
  }
  if (!label.value || presets.some((x) => displayLabel(x) === label.value || x.label === label.value)) {
    label.value = displayLabel(p);
  }
}

const valid = computed(() => {
  if (isLocal.value) return !!model.value && !!label.value.trim();
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
  const options = { ...(props.profile?.options ?? {}) };
  if (isLocal.value) options["load_policy"] = loadPolicy.value;
  if (canConfigureReasoning.value && isReasoningEffort(reasoningEffort.value)) {
    options["reasoning_effort"] = reasoningEffort.value;
    options["enable_thinking"] = reasoningEffort.value !== "none";
  } else {
    delete options["reasoning_effort"];
    delete options["enable_thinking"];
  }
  const profile: ProviderProfile = {
    id,
    capability: props.profile?.capability ?? props.capability,
    kind: kind.value,
    label: label.value.trim(),
    base_url: isLocal.value ? "" : baseUrl.value.trim().replace(/\/+$/, ""),
    model: model.value.trim(),
    credentials: props.profile?.credentials ?? {},
    extra_headers: props.profile?.extra_headers ?? {},
    extra_form: props.profile?.extra_form ?? {},
    timeout_ms: props.profile?.timeout_ms ?? 30000,
    options,
  };
  const r = await commands.upsertProfile(profile);
  if (r.status !== "ok") {
    testResult.value = profileErrorMessage(r.error);
    testOk.value = false;
    saving.value = false;
    return null;
  }
  if (isLocal.value) {
    // 本地推理无凭据（mockup 2.7）
  } else if (isVolc.value) {
    if (appKey.value.trim()) {
      if (!(await saveSecret(id, "app_key", appKey.value.trim()))) {
        saving.value = false;
        return null;
      }
    }
    if (accessToken.value.trim()) {
      if (!(await saveSecret(id, "access_token", accessToken.value.trim()))) {
        saving.value = false;
        return null;
      }
    }
  } else if (apiKey.value.trim()) {
    if (!(await saveSecret(id, "api_key", apiKey.value.trim()))) {
      saving.value = false;
      return null;
    }
  }
  if (isNew.value && props.assignTo) {
    const activated = await commands.activateProfile(props.assignTo, id);
    if (activated.status !== "ok") {
      testResult.value = profileErrorMessage(activated.error);
      testOk.value = false;
      saving.value = false;
      return null;
    }
  }
  saving.value = false;
  return id;
}

async function saveAndBack() {
  const id = await save();
  if (id) emit("saved");
}

async function testConnection() {
  testResult.value = t("settings.profile.testing");
  testOk.value = false;
  const id = await save();
  if (!id) {
    if (testResult.value === t("settings.profile.testing")) {
      testResult.value = t("settings.profile.fill_form");
    }
    return;
  }
  const r = await commands.testProfile(id);
  if (r.status === "ok") {
    testResult.value = t("settings.profile.test_pass", { ms: r.data });
    testOk.value = true;
  } else {
    testResult.value = profileErrorMessage(r.error);
    testOk.value = false;
  }
}

async function deleteProfile() {
  if (!props.profile) return;
  await commands.deleteProfile(props.profile.id);
  emit("saved");
}

onMounted(async () => {
  if (isLocal.value) await loadLocalModels();
  unlistenProgress = await events.localDownloadProgressEvent.listen((e) => {
    if (e.payload.model_id !== model.value) return;
    if (e.payload.done) {
      downloadPct.value = null;
      void loadLocalModels();
    } else if (e.payload.bytes_total > 0) {
      downloadPct.value = Math.round((e.payload.bytes_done / e.payload.bytes_total) * 100);
    }
  });
});
onUnmounted(() => unlistenProgress?.());
</script>

<template>
  <div>
    <p class="back"><a @click="emit('back')">← {{ t("settings.nav_providers") }}</a></p>
    <h5 class="page-title">
      {{ isNew ? t("settings.profile.title_new") : t("settings.profile.title_edit") }} —
      {{ t(CAPABILITY_LABEL_KEY[capability]) }}
    </h5>

    <FormRow :label="t('settings.profile.preset')">
      <Select
        :model-value="presetId"
        :options="presets.map((p) => ({ value: p.id, label: displayLabel(p) }))"
        @update:model-value="applyPreset"
      />
    </FormRow>
    <FormRow :label="t('settings.profile.name')">
      <span class="w280"><Input v-model="label" :placeholder="t('settings.profile.name_ph')" /></span>
    </FormRow>

    <!-- 本地档案编辑态（mockup 2.7）：模型下拉 + 加载策略；无端点/密钥 -->
    <template v-if="isLocal">
      <FormRow :label="t('settings.profile.model')">
        <span class="local-model">
          <Select
            v-model="model"
            :options="localOptions.length ? localOptions : [{ value: '', label: t('settings.profile.local_no_models') }]"
            :disabled="!localOptions.length"
          />
          <Button
            v-if="selectedLocal && !selectedLocal.downloaded && downloadPct === null"
            size="sm"
            @click="downloadSelected"
          >
            {{ t("settings.profile.local_download") }}
          </Button>
          <span v-if="downloadPct !== null" class="pbar"><i :style="{ width: `${downloadPct}%` }" /></span>
        </span>
      </FormRow>
      <FormRow :label="t('settings.profile.load_policy')" :hint="t('settings.profile.load_policy_hint')">
        <Select
          v-model="loadPolicy"
          :options="[
            { value: 'resident', label: t('settings.profile.load_policy_resident') },
            { value: 'unload_after_use', label: t('settings.profile.load_policy_unload') },
          ]"
        />
      </FormRow>
      <FormRow
        v-if="canConfigureReasoning"
        :label="t('settings.profile.reasoning_effort')"
        :hint="t('settings.profile.reasoning_effort_hint')"
      >
        <Select v-model="reasoningEffort" :options="reasoningOptions" />
      </FormRow>
      <p class="local-note">{{ t("settings.profile.local_note") }}</p>
    </template>

    <!-- 云端编辑态（mockup 2.6） -->
    <template v-else>
      <FormRow v-if="!isVolc" label="Base URL">
        <span class="w280"><Input v-model="baseUrl" mono placeholder="https://api.example.com/v1" /></span>
      </FormRow>
      <FormRow v-else :label="t('settings.profile.endpoint')" :hint="t('settings.profile.endpoint_hint')">
        <span class="w280"><Input v-model="baseUrl" mono :placeholder="t('settings.profile.endpoint_ph')" /></span>
      </FormRow>
      <FormRow :label="t('settings.profile.model')">
        <span class="w280"><Input v-model="model" mono :placeholder="t('settings.profile.model_ph')" /></span>
      </FormRow>
      <FormRow v-if="capability !== 'stt'" :label="t('settings.profile.api_format')">
        <Select
          v-model="kind"
          :options="[
            { value: 'chat_completions', label: t('settings.profile.kind_chat') },
            { value: 'responses', label: t('settings.profile.kind_responses') },
          ]"
        />
      </FormRow>
      <FormRow
        v-if="canConfigureReasoning"
        :label="t('settings.profile.reasoning_effort')"
        :hint="t('settings.profile.reasoning_effort_hint')"
      >
        <Select v-model="reasoningEffort" :options="reasoningOptions" />
      </FormRow>
      <template v-if="isVolc">
        <FormRow
          label="APP ID"
          :hint="hasExistingVolcKeys ? t('settings.profile.key_saved_hint') : t('settings.profile.app_id_hint')"
        >
          <span class="w280"><SecretInput v-model="appKey" /></span>
        </FormRow>
        <FormRow
          label="Access Token"
          :hint="hasExistingVolcKeys ? undefined : t('settings.profile.token_hint')"
        >
          <span class="w280"><SecretInput v-model="accessToken" /></span>
        </FormRow>
      </template>
      <FormRow
        v-else
        :label="t('settings.profile.api_key')"
        :hint="hasExistingKey ? t('settings.profile.key_saved_hint') : undefined"
      >
        <span class="w280"><SecretInput v-model="apiKey" /></span>
      </FormRow>
    </template>

    <div class="actions">
      <Button variant="primary" :disabled="!valid || saving" @click="saveAndBack">{{ t("actions.save") }}</Button>
      <Button @click="testConnection">{{ t("settings.profile.test_connection") }}</Button>
      <span class="spacer" />
      <Button v-if="!isNew" variant="danger" @click="deleteProfile">{{ t("settings.profile.delete_profile") }}</Button>
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
.local-model {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}
/* 进度条 = --text-1 实底，无彩色（mockup 步骤 3b 同款纪律） */
.pbar {
  width: 90px;
  height: 4px;
  border-radius: 99px;
  background: var(--border);
  overflow: hidden;
  display: inline-block;
}
.pbar i {
  display: block;
  height: 100%;
  background: var(--text-1);
  transition: width 0.3s;
}
.local-note {
  font-size: 11.5px;
  color: var(--text-3);
  margin-top: 6px;
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
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}
.test-result.ok {
  color: var(--success);
}
</style>
