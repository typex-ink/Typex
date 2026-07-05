<script setup lang="ts">
// ProviderCard 编辑子页（mockup 2.6）：预设下拉 → 按 kind 动态字段 → 保存/测试/删除
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
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

const { t } = useI18n();

const SLOT_LABEL_KEY: Record<SlotKind, string> = {
  stt: "settings.providers.slot_stt",
  polish: "settings.providers.slot_polish",
  translate: "settings.providers.slot_translate",
  assistant: "settings.providers.slot_assistant",
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

function displayLabel(p: (typeof presets)[number]): string {
  return p.labelKey ? t(p.labelKey) : p.label;
}

function applyPreset(id: string) {
  presetId.value = id;
  const p = presets.find((x) => x.id === id);
  if (!p) return;
  if (p.base_url) baseUrl.value = p.base_url;
  kind.value = p.kind;
  if (p.models[0]) model.value = p.models[0];
  if (!label.value || presets.some((x) => displayLabel(x) === label.value || x.label === label.value)) {
    label.value = displayLabel(p);
  }
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
  testResult.value = t("settings.profile.testing");
  testOk.value = false;
  const id = await save();
  if (!id) {
    testResult.value = t("settings.profile.fill_form");
    return;
  }
  const r = await commands.testProfile(id);
  if (r.status === "ok") {
    testResult.value = t("settings.profile.test_pass", { ms: r.data });
    testOk.value = true;
  } else {
    const KEYS: Record<string, string> = {
      auth_error: "settings.profile.err_auth_error",
      network_error: "settings.profile.err_network_error",
      timeout: "settings.profile.err_timeout",
      invalid_request: "settings.profile.err_invalid_request",
      server_error: "settings.profile.err_server_error",
    };
    const key = KEYS[r.error.code];
    testResult.value = `✗ ${key ? t(key) : r.error.code}`;
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
    <p class="back"><a @click="emit('back')">← {{ t("settings.nav_providers") }}</a></p>
    <h5 class="page-title">
      {{ isNew ? t("settings.profile.title_new") : t("settings.profile.title_edit") }} —
      {{ t(SLOT_LABEL_KEY[slotKind]) }}
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
    <FormRow v-if="!isVolc" label="Base URL">
      <span class="w280"><Input v-model="baseUrl" mono placeholder="https://api.example.com/v1" /></span>
    </FormRow>
    <FormRow v-else :label="t('settings.profile.endpoint')" :hint="t('settings.profile.endpoint_hint')">
      <span class="w280"><Input v-model="baseUrl" mono :placeholder="t('settings.profile.endpoint_ph')" /></span>
    </FormRow>
    <FormRow :label="t('settings.profile.model')">
      <span class="w280"><Input v-model="model" mono :placeholder="t('settings.profile.model_ph')" /></span>
    </FormRow>
    <FormRow v-if="slotKind !== 'stt'" :label="t('settings.profile.api_format')">
      <Select
        v-model="kind"
        :options="[
          { value: 'chat_completions', label: t('settings.profile.kind_chat') },
          { value: 'responses', label: t('settings.profile.kind_responses') },
        ]"
      />
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
