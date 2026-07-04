<script setup lang="ts">
// 设置窗口 720×520：左导航 160px + 内容区（05 §5 / mockup §2）
import { onMounted, ref } from "vue";
import { useSettingsStore } from "@/stores/settings";
import GeneralPage from "./pages/GeneralPage.vue";
import DictationPage from "./pages/DictationPage.vue";
import TranslationPage from "./pages/TranslationPage.vue";
import AssistantPage from "./pages/AssistantPage.vue";
import ProvidersPage from "./pages/ProvidersPage.vue";
import HotkeysPage from "./pages/HotkeysPage.vue";
import HistoryPage from "./pages/HistoryPage.vue";
import DiagnosticsPage from "./pages/DiagnosticsPage.vue";
import AboutPage from "./pages/AboutPage.vue";

const pages = [
  { id: "general", label: "通用", comp: GeneralPage },
  { id: "dictation", label: "听写", comp: DictationPage },
  { id: "translation", label: "翻译", comp: TranslationPage },
  { id: "assistant", label: "助手", comp: AssistantPage },
  { id: "providers", label: "模型服务", comp: ProvidersPage },
  { id: "hotkeys", label: "快捷键", comp: HotkeysPage },
  { id: "history", label: "历史", comp: HistoryPage },
  { id: "diagnostics", label: "诊断", comp: DiagnosticsPage },
  { id: "about", label: "关于", comp: AboutPage },
] as const;

const active = ref<string>("general");
const store = useSettingsStore();

onMounted(() => store.load());
</script>

<template>
  <div class="settings-root">
    <nav class="nav">
      <div
        v-for="p in pages"
        :key="p.id"
        class="nav-item"
        :class="{ on: active === p.id }"
        @click="active = p.id"
      >
        {{ p.label }}
      </div>
    </nav>
    <main class="content">
      <component :is="pages.find((p) => p.id === active)!.comp" v-if="store.loaded" />
    </main>
  </div>
</template>

<style scoped>
.settings-root {
  display: flex;
  width: 100vw;
  height: 100vh;
  background: var(--surface);
  overflow: hidden;
}
.nav {
  width: 160px;
  flex-shrink: 0;
  background: var(--surface-2);
  padding: 10px 8px;
  font-size: 12.5px;
  overflow-y: auto;
}
.nav-item {
  padding: 7px 12px;
  border-radius: var(--radius-control);
  color: var(--text-2);
  margin-bottom: 2px;
  cursor: pointer;
}
.nav-item:hover {
  background: var(--sel-bg);
}
/* 选中态 = 灰底 + 600 字重（禁止反色实底，ADR-18） */
.nav-item.on {
  background: var(--sel-bg);
  color: var(--text-1);
  font-weight: 600;
}
.content {
  flex: 1;
  padding: 18px 20px;
  overflow-y: auto;
}
</style>
