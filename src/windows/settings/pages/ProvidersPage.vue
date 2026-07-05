<script setup lang="ts">
// 模型服务页（05 §5.1 / mockup 2.5–2.8）：四槽位 ProviderCard + 编辑子页 + 共用开关
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import Toggle from "@/components/Toggle.vue";
import ProviderCard from "@/components/ProviderCard.vue";
import ProfileEditor from "./ProfileEditor.vue";
import { useSettingsStore } from "@/stores/settings";
import { commands, type SlotKind, type ProviderProfile } from "@/ipc/bindings";

const { t } = useI18n();
const store = useSettingsStore();

const SLOTS: { slot: SlotKind; key: string }[] = [
  { slot: "stt", key: "settings.providers.slot_stt" },
  { slot: "polish", key: "settings.providers.slot_polish" },
  { slot: "translate", key: "settings.providers.slot_translate" },
  { slot: "assistant", key: "settings.providers.slot_assistant" },
];

// 编辑子页状态（05 §5.1：同一内容区内切换，顶部 ← 返回）
const editing = ref<{ slot: SlotKind; profile: ProviderProfile | null } | null>(null);

const profiles = computed(() => store.settings?.profiles ?? []);

function activeProfileOf(slot: SlotKind): ProviderProfile | null {
  const id = store.settings?.slots[slot]?.active_profile;
  return profiles.value.find((p) => p.id === id) ?? null;
}

function alternativesOf(slot: SlotKind): ProviderProfile[] {
  return profiles.value.filter((p) => p.slots.includes(slot));
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

function onSaved() {
  editing.value = null;
  store.load();
}
</script>

<template>
  <ProfileEditor
    v-if="editing"
    :slot-kind="editing.slot"
    :profile="editing.profile"
    @back="editing = null"
    @saved="onSaved"
  />
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
        @edit="editing = { slot, profile: activeProfileOf(slot) }"
        @create="editing = { slot, profile: null }"
        @switch="(id) => switchProfile(slot, id)"
      />
    </template>
  </div>
</template>

<style scoped>
.page-title {
  font-size: 15px;
  margin-bottom: 14px;
  font-weight: 600;
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
