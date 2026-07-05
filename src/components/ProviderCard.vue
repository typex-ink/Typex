<script setup lang="ts">
// ProviderCard（04 §7 / mockup 2.5）：label + 模型 + 状态 + 测试/编辑/切换
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import Button from "@/components/Button.vue";
import { commands, type ProviderProfile } from "@/ipc/bindings";

const { t, te } = useI18n();

const props = defineProps<{
  profile: ProviderProfile | null;
  active?: boolean;
  /** 该槽位全部可切换档案 */
  alternatives: ProviderProfile[];
}>();

const emit = defineEmits<{
  edit: [];
  create: [];
  switch: [profileId: string];
}>();

const testing = ref(false);
const testResult = ref<string | null>(null);
const testError = ref(false);
const switchOpen = ref(false);

async function runTest() {
  if (!props.profile) return;
  testing.value = true;
  testResult.value = null;
  const r = await commands.testProfile(props.profile.id);
  testing.value = false;
  if (r.status === "ok") {
    testResult.value = `✓ ${r.data}ms`;
    testError.value = false;
  } else {
    testResult.value = errText(r.error.code);
    testError.value = true;
  }
}

function errText(code: string): string {
  const key = `components.provider_card.err_${code}`;
  return `✗ ${te(key) ? t(key) : code}`;
}
</script>

<template>
  <div class="prov" :class="{ on: active }">
    <template v-if="profile">
      <div class="logo">{{ profile.label.charAt(0).toUpperCase() }}</div>
      <div class="meta">
        <b>{{ profile.label }}</b> · {{ profile.model }}<br />
        <small>{{ profile.kind }}</small>
      </div>
      <span v-if="testResult" class="lat" :class="{ err: testError }">{{ testResult }}</span>
      <Button size="sm" :disabled="testing" @click="runTest">
        {{ testing ? t("components.provider_card.testing") : t("actions.test") }}
      </Button>
      <Button size="sm" @click="emit('edit')">{{ t("actions.edit") }}</Button>
      <span class="switch-wrap">
        <Button variant="ghost" size="sm" @click="switchOpen = !switchOpen">
          {{ t("actions.switch") }} {{ switchOpen ? "▴" : "▾" }}
        </Button>
        <div v-if="switchOpen" class="menu">
          <div class="st">{{ t("components.provider_card.switch_menu") }}</div>
          <hr />
          <div
            v-for="alt in alternatives"
            :key="alt.id"
            class="it"
            :class="{ cur: alt.id === profile.id }"
            @click="
              switchOpen = false;
              if (alt.id !== profile.id) emit('switch', alt.id);
            "
          >
            <span>{{ alt.id === profile.id ? "✓ " : "　" }}{{ alt.label }}</span>
          </div>
          <hr />
          <div
            class="it"
            @click="
              switchOpen = false;
              emit('create');
            "
          >
            <span>{{ t("components.provider_card.new_config") }}</span>
          </div>
        </div>
      </span>
    </template>
    <template v-else>
      <div class="logo empty">?</div>
      <div class="meta">
        <b class="unconfigured">{{ t("components.provider_card.unconfigured") }}</b><br />
        <small>{{ t("components.provider_card.unconfigured_hint") }}</small>
      </div>
      <Button variant="primary" size="sm" @click="emit('create')">{{ t("components.provider_card.configure") }}</Button>
    </template>
  </div>
</template>

<style scoped>
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
/* 选中卡片 = 灰底 + 加重发丝线（禁止反色实底） */
.prov.on {
  background: var(--sel-bg);
  border-color: var(--border-2);
}
.logo {
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
.meta {
  flex: 1;
  line-height: 1.45;
  min-width: 0;
}
.meta small {
  color: var(--text-3);
  font-size: 11px;
}
.unconfigured {
  color: var(--text-2);
}
.lat {
  font-size: 11px;
  color: var(--text-2);
  border: 1px solid var(--border-2);
  padding: 2px 8px;
  border-radius: 99px;
  font-family: var(--font-mono);
  white-space: nowrap;
}
.lat.err {
  color: var(--error);
  border-color: var(--error);
}
.switch-wrap {
  position: relative;
}
.menu {
  position: absolute;
  right: 0;
  top: 30px;
  width: 250px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 10px;
  box-shadow: var(--shadow);
  padding: 5px;
  font-size: 12.5px;
  z-index: 10;
}
.menu .st {
  padding: 6px 10px;
  color: var(--text-2);
  font-size: 12px;
}
.menu hr {
  border: none;
  border-top: 1px solid var(--border);
  margin: 4px 6px;
}
.menu .it {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 6px 10px;
  border-radius: 6px;
  cursor: pointer;
}
.menu .it:hover {
  background: var(--sel-bg);
}
.menu .it.cur span {
  font-weight: 600;
}
</style>
