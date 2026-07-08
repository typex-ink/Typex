<script setup lang="ts">
// 模型服务页（05 §5.1 / mockup 2.5–2.9）：功能分配 + 底部服务配置池
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Button from "@/components/Button.vue";
import ProviderCard from "@/components/ProviderCard.vue";
import ProfileEditor from "./ProfileEditor.vue";
import ModelManager from "./ModelManager.vue";
import { useSettingsStore } from "@/stores/settings";
import { formatBytes } from "@/shared/format";
import {
  commands,
  type LocalModelInfo,
  type ProviderCapability,
  type SlotKind,
  type ProviderProfile,
} from "@/ipc/bindings";

const { t } = useI18n();
const store = useSettingsStore();

const SLOTS: { slot: SlotKind; key: string }[] = [
  { slot: "stt", key: "settings.providers.slot_stt" },
  { slot: "polish", key: "settings.providers.slot_polish" },
  { slot: "translate", key: "settings.providers.slot_translate" },
  { slot: "assistant", key: "settings.providers.slot_assistant" },
];
const CAPABILITIES: ProviderCapability[] = ["stt", "llm"];

// 子页状态（05 §5.1：同一内容区内切换，顶部 ← 返回）
const editing = ref<{
  capability: ProviderCapability;
  profile: ProviderProfile | null;
  assignTo?: SlotKind | null;
} | null>(null);
const managing = ref(false);

const profiles = computed(() => store.settings?.profiles ?? []);
// 本地模型状态（本地卡片副标题：引擎 + 已下载·体积 / 未下载）
const localModels = ref<LocalModelInfo[]>([]);
// 底部摘要行（mockup 2.5）：已下载模型列表；feature 未启用时 localModels 恒空 → 整行隐藏
const downloadedModels = computed(() => localModels.value.filter((m) => m.downloaded));
const localAvailable = computed(() => localModels.value.length > 0);

function activeProfileOf(slot: SlotKind): ProviderProfile | null {
  const id = store.settings?.slots[slot]?.active_profile;
  return profiles.value.find((p) => p.id === id) ?? null;
}

function capabilityOf(slot: SlotKind): ProviderCapability {
  return slot === "stt" ? "stt" : "llm";
}

function alternativesOf(slot: SlotKind): ProviderProfile[] {
  const capability = capabilityOf(slot);
  return profiles.value.filter((p) => p.capability === capability);
}

function usedSlotLabels(profileId: string): string[] {
  const s = store.settings;
  if (!s) return [];
  return SLOTS
    .filter(({ slot }) => s.slots[slot]?.active_profile === profileId)
    .map(({ key }) => t(key));
}

function localEngineLabel(engine: string): string {
  if (engine === "llama") return "llama.cpp";
  if (engine === "sherpa_whisper") return "sherpa-onnx Whisper";
  if (engine === "sherpa") return "sherpa-onnx";
  return engine;
}

/** 本地档案卡片副标题（mockup 2.8：`local · 已下载 · 1.3 GB · 离线`）；云端显示 adapter。 */
function subtitleOfProfile(p: ProviderProfile | null): string | undefined {
  if (!p) return undefined;
  let base: string = p.kind;
  if (p.kind === "local") {
    const m = localModels.value.find((x) => x.id === p.model);
    if (!m) base = t("settings.providers.local_subtitle_unknown");
    else {
      const status = m.downloaded
        ? t("settings.providers.local_downloaded", { size: formatBytes(m.bytes) })
        : t("settings.providers.local_not_downloaded");
      base = `${localEngineLabel(m.engine)} · ${status} · ${t("settings.providers.local_offline")}`;
    }
  }
  const used = usedSlotLabels(p.id);
  return used.length
    ? `${base} · ${t("settings.models.in_use", { slots: used.join(" · ") })}`
    : base;
}

function subtitleOf(slot: SlotKind): string | undefined {
  return subtitleOfProfile(activeProfileOf(slot));
}

function editProfile(profile: ProviderProfile) {
  editing.value = { capability: profile.capability, profile };
}

function createProfile(capability: ProviderCapability, assignTo?: SlotKind) {
  editing.value = { capability, profile: null, assignTo };
}

function activeInAnySlot(profileId: string): boolean {
  return usedSlotLabels(profileId).length > 0;
}

function poolLabel(capability: ProviderCapability): string {
  return t(capability === "stt" ? "settings.providers.service_stt" : "settings.providers.service_llm");
}

function profilesOf(capability: ProviderCapability): ProviderProfile[] {
  return profiles.value.filter((p) => p.capability === capability);
}

function subtitleOfPoolProfile(p: ProviderProfile): string | undefined {
  return subtitleOfProfile(p);
}

function createForSlot(slot: SlotKind) {
  createProfile(capabilityOf(slot), slot);
}

function editSlotProfile(slot: SlotKind) {
  const p = activeProfileOf(slot);
  if (p) editProfile(p);
  else createForSlot(slot);
}

function emptyHint(capability: ProviderCapability): string {
  return t(capability === "stt" ? "settings.providers.pool_empty_stt" : "settings.providers.pool_empty_llm");
}

function newButtonLabel(capability: ProviderCapability): string {
  return t(capability === "stt" ? "settings.providers.new_stt_service" : "settings.providers.new_llm_service");
}

async function switchProfile(slot: SlotKind, id: string) {
  await commands.activateProfile(slot, id);
  await store.load();
}

async function loadLocalModels() {
  const r = await commands.listLocalModels();
  if (r.status === "ok") localModels.value = r.data;
}

function onSaved() {
  editing.value = null;
  store.load();
  loadLocalModels();
}

onMounted(loadLocalModels);
</script>

<template>
  <ProfileEditor
    v-if="editing"
    :capability="editing.capability"
    :profile="editing.profile"
    :assign-to="editing.assignTo"
    @back="editing = null"
    @saved="onSaved"
  />
  <ModelManager v-else-if="managing" @back="managing = false; loadLocalModels()" />
  <div v-else>
    <h5 class="page-title">{{ t("settings.nav_providers") }}</h5>
    <div class="section-head">{{ t("settings.providers.assignments_title") }}</div>
    <template v-for="{ slot, key } in SLOTS" :key="slot">
      <div class="slot-h">
        <span>{{ t(key) }}</span>
      </div>
      <ProviderCard
        :profile="activeProfileOf(slot)"
        :active="!!activeProfileOf(slot)"
        :alternatives="alternativesOf(slot)"
        :subtitle="subtitleOf(slot)"
        @edit="editSlotProfile(slot)"
        @create="createForSlot(slot)"
        @switch="(id) => switchProfile(slot, id)"
      />
    </template>
    <!-- 底部摘要行（mockup 2.5）：已下载模型 + 「管理…」入口；feature 未启用时隐藏 -->
    <p v-if="localAvailable" class="models-line">
      <template v-if="downloadedModels.length">
        {{ t("settings.providers.downloaded_prefix") }}
        {{ downloadedModels.map((m) => `${m.display_name} ${formatBytes(m.bytes)}`).join(" · ") }}
        ·
      </template>
      <template v-else>{{ t("settings.providers.no_downloaded") }} · </template>
      <a class="manage" @click="managing = true">{{ t("settings.providers.manage_models") }}</a>
    </p>

    <div class="pool-head">
      <span>{{ t("settings.providers.pool_title") }}</span>
      <span class="pool-actions">
        <Button size="sm" @click="createProfile('stt')">{{ newButtonLabel("stt") }}</Button>
        <Button size="sm" @click="createProfile('llm')">{{ newButtonLabel("llm") }}</Button>
      </span>
    </div>
    <template v-for="capability in CAPABILITIES" :key="capability">
      <div class="slot-h pool-subhead">
        <span>{{ poolLabel(capability) }}</span>
      </div>
      <p v-if="!profilesOf(capability).length" class="empty">{{ emptyHint(capability) }}</p>
      <ProviderCard
        v-for="profile in profilesOf(capability)"
        :key="profile.id"
        :profile="profile"
        :active="activeInAnySlot(profile.id)"
        :subtitle="subtitleOfPoolProfile(profile)"
        @edit="editProfile(profile)"
        @create="createProfile(profile.capability)"
      />
    </template>
  </div>
</template>

<style scoped>
.page-title {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 14px;
}
.models-line {
  font-size: 11px;
  color: var(--text-3);
  margin-top: 12px;
  margin-bottom: 16px;
}
.manage {
  text-decoration: underline;
  cursor: pointer;
  color: var(--text-2);
}
.slot-h {
  font-size: 11px;
  color: var(--text-3);
  letter-spacing: 0.06em;
  margin: 16px 0 8px;
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.slot-h:first-of-type {
  margin-top: 0;
}
.section-head,
.pool-head {
  font-size: 12px;
  color: var(--text-2);
  font-weight: 600;
  margin: 12px 0 8px;
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.pool-head {
  margin-top: 18px;
}
.pool-actions {
  display: inline-flex;
  gap: 8px;
}
.pool-subhead {
  margin-top: 10px;
}
.empty {
  font-size: 12px;
  color: var(--text-3);
  margin: 4px 0 8px;
}
</style>
