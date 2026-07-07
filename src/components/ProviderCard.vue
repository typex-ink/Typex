<script setup lang="ts">
// ProviderCard（04 §7 / mockup 2.5）：label + 模型 + 状态 + 测试/编辑/切换
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import Button from "@/components/Button.vue";
import { commands, type ProviderProfile } from "@/ipc/bindings";

const { t, te } = useI18n();

const props = withDefaults(defineProps<{
  profile: ProviderProfile | null;
  active?: boolean;
  /** 该槽位全部可切换档案 */
  alternatives?: ProviderProfile[];
  /** 副标题覆盖（本地档案显示引擎与模型状态，CP-8.7；缺省显示 kind） */
  subtitle?: string;
}>(), {
  active: false,
  alternatives: () => [],
});

const emit = defineEmits<{
  edit: [];
  create: [];
  switch: [profileId: string];
}>();

const testing = ref(false);
const testResult = ref<string | null>(null);
const testError = ref(false);
const switchOpen = ref(false);
const testTooltipId = `provider-test-${Math.random().toString(36).slice(2)}`;
const showSwitch = computed(() => props.alternatives.length > 0);
const configureText = computed(() =>
  showSwitch.value
    ? `${t("components.provider_card.configure_select")} ${switchOpen.value ? "▴" : "▾"}`
    : t("components.provider_card.configure"),
);

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
    const upstream = r.error.message?.trim();
    testResult.value = `${errText(r.error.code)}${upstream ? `：${upstream}` : ""}`;
    testError.value = true;
  }
}

function errText(code: string): string {
  const key = `components.provider_card.err_${code}`;
  return `✗ ${te(key) ? t(key) : code}`;
}

function chooseProfile(profileId: string) {
  switchOpen.value = false;
  if (props.profile?.id !== profileId) emit("switch", profileId);
}

function createProfile() {
  switchOpen.value = false;
  emit("create");
}

function configureEmpty() {
  if (showSwitch.value) {
    switchOpen.value = !switchOpen.value;
  } else {
    emit("create");
  }
}
</script>

<template>
  <div class="prov" :class="{ on: active }">
    <template v-if="profile">
      <div class="logo">{{ profile.label.charAt(0).toUpperCase() }}</div>
      <div class="meta">
        <b>{{ profile.label }}</b> · {{ profile.model }}<br />
        <small>{{ subtitle ?? profile.kind }}</small>
      </div>
      <span v-if="testResult && !testError" class="lat">{{ testResult }}</span>
      <span class="test-wrap">
        <Button
          :variant="testError ? 'danger' : 'secondary'"
          size="sm"
          :disabled="testing"
          :title="testError && testResult ? testResult : undefined"
          :aria-describedby="testError && testResult ? testTooltipId : undefined"
          @click="runTest"
        >
          {{
            testing
              ? t("components.provider_card.testing")
              : testError
                ? t("components.provider_card.test_failed")
                : t("actions.test")
          }}
        </Button>
        <span
          v-if="testError && testResult"
          :id="testTooltipId"
          class="test-tip"
          role="tooltip"
        >
          {{ testResult }}
        </span>
      </span>
      <Button size="sm" @click="emit('edit')">{{ t("actions.edit") }}</Button>
      <span v-if="showSwitch" class="switch-wrap">
        <Button variant="ghost" size="sm" @click="switchOpen = !switchOpen">
          {{ t("actions.switch") }} {{ switchOpen ? "▴" : "▾" }}
        </Button>
        <div v-if="switchOpen" class="menu">
          <div
            v-for="alt in alternatives"
            :key="alt.id"
            class="it"
            :class="{ cur: alt.id === profile.id }"
            @click="chooseProfile(alt.id)"
          >
            <span>{{ alt.id === profile.id ? "✓ " : "　" }}{{ alt.label }}</span>
          </div>
          <hr />
          <div class="it" @click="createProfile">
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
      <span class="switch-wrap">
        <Button variant="primary" size="sm" @click="configureEmpty">
          {{ configureText }}
        </Button>
        <div v-if="showSwitch && switchOpen" class="menu">
          <div
            v-for="alt in alternatives"
            :key="alt.id"
            class="it"
            @click="chooseProfile(alt.id)"
          >
            <span>　{{ alt.label }}</span>
          </div>
          <hr />
          <div class="it" @click="createProfile">
            <span>{{ t("components.provider_card.new_config") }}</span>
          </div>
        </div>
      </span>
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
.test-wrap {
  position: relative;
  display: inline-flex;
  flex-shrink: 0;
}
.test-tip {
  position: absolute;
  right: 0;
  top: calc(100% + 8px);
  width: min(360px, 48vw);
  max-width: calc(100vw - 48px);
  padding: 8px 10px;
  border: 1px solid var(--error);
  border-radius: var(--radius-control);
  background: var(--surface);
  color: var(--error);
  box-shadow: var(--shadow);
  font-size: 12px;
  line-height: 1.45;
  font-family: var(--font-mono);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  opacity: 0;
  transform: translateY(-2px);
  pointer-events: none;
  transition: opacity 0.12s ease-out, transform 0.12s ease-out;
  z-index: 20;
}
.test-wrap:hover .test-tip,
.test-wrap:focus-within .test-tip {
  opacity: 1;
  transform: translateY(0);
}
@media (prefers-reduced-motion: reduce) {
  .test-tip {
    transition: none;
  }
}
.switch-wrap {
  position: relative;
  flex-shrink: 0;
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
