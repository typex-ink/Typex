<script setup lang="ts">
// 模型服务页（05 §5.1 / mockup 2.5–2.9）：四槽位 ProviderCard + 编辑子页 + 模型管理子页 + 共用开关
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Toggle from "@/components/Toggle.vue";
import ProviderCard from "@/components/ProviderCard.vue";
import ProfileEditor from "./ProfileEditor.vue";
import ModelManager from "./ModelManager.vue";
import { useSettingsStore } from "@/stores/settings";
import { formatBytes } from "@/shared/format";
import { commands, type LocalModelInfo, type SlotKind, type ProviderProfile } from "@/ipc/bindings";

const { t } = useI18n();
const store = useSettingsStore();

const SLOTS: { slot: SlotKind; key: string }[] = [
  { slot: "stt", key: "settings.providers.slot_stt" },
  { slot: "polish", key: "settings.providers.slot_polish" },
  { slot: "translate", key: "settings.providers.slot_translate" },
  { slot: "assistant", key: "settings.providers.slot_assistant" },
];

// 子页状态（05 §5.1：同一内容区内切换，顶部 ← 返回）
const editing = ref<{ slot: SlotKind; profile: ProviderProfile | null } | null>(null);
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

function alternativesOf(slot: SlotKind): ProviderProfile[] {
  return profiles.value.filter((p) => p.slots.includes(slot));
}

/** 本地档案卡片副标题（mockup 2.8：`local · 已下载 · 1.3 GB · 离线`）；云端返回 undefined 走默认 */
function subtitleOf(slot: SlotKind): string | undefined {
  const p = activeProfileOf(slot);
  if (!p || p.kind !== "local") return undefined;
  const m = localModels.value.find((x) => x.id === p.model);
  if (!m) return t("settings.providers.local_subtitle_unknown");
  const status = m.downloaded
    ? t("settings.providers.local_downloaded", { size: formatBytes(m.bytes) })
    : t("settings.providers.local_not_downloaded");
  return `${m.engine} · ${status} · ${t("settings.providers.local_offline")}`;
}

// 「与翻译共用」开关（03 §5 共用规则：整理槽指向翻译槽的档案）
const polishSharesTranslate = computed({
  get: () => {
    const p = store.settings?.slots.polish?.active_profile;
    const t = store.settings?.slots.translate?.active_profile;
    return !!p && p === t;
  },
  set: (share) => {
    if (share) {
      const t = store.settings?.slots.translate?.active_profile;
      if (t) commands.activateProfile("polish", t).then(() => store.load());
    }
  },
});

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
    :slot-kind="editing.slot"
    :profile="editing.profile"
    @back="editing = null"
    @saved="onSaved"
  />
  <ModelManager v-else-if="managing" @back="managing = false; loadLocalModels()" />
  <div v-else>
    <h5 class="page-title">{{ t("settings.nav_providers") }}</h5>
    <template v-for="{ slot, key } in SLOTS" :key="slot">
      <div class="slot-h">
        <span>{{ t(key) }}</span>
        <span v-if="slot === 'polish'" class="share">
          {{ t("settings.providers.share_with_translate") }} <Toggle v-model="polishSharesTranslate" />
        </span>
      </div>
      <ProviderCard
        v-if="!(slot === 'polish' && polishSharesTranslate)"
        :profile="activeProfileOf(slot)"
        :active="!!activeProfileOf(slot)"
        :alternatives="alternativesOf(slot)"
        :subtitle="subtitleOf(slot)"
        @edit="editing = { slot, profile: activeProfileOf(slot) }"
        @create="editing = { slot, profile: null }"
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
.share {
  font-size: 11.5px;
  color: var(--text-2);
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
