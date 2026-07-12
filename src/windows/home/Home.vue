<script setup lang="ts">
// 主页窗口 880×560（05 §8 / ADR-19）：侧边栏 + 首页/历史记录页签
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Kbd from "@/components/Kbd.vue";
import Button from "@/components/Button.vue";
import Input from "@/components/Input.vue";
import { commands, events, type HistoryItem, type HistoryStats } from "@/ipc/bindings";
import { useSettingsStore } from "@/stores/settings";
import { usePlatform } from "@/composables/usePlatform";

const { t, te } = useI18n();
const store = useSettingsStore();
const { defaultHotkeys, keyLabel } = usePlatform();
const DICTIONARY_MAX_TERMS = 100;
const DICTIONARY_MAX_TERM_CHARS = 50;

const tab = ref<"overview" | "history" | "dictionary">("overview");
const stats = ref<HistoryStats | null>(null);
const recent = ref<HistoryItem[]>([]);
const items = ref<HistoryItem[]>([]);
const search = ref("");
const expanded = ref<number | null>(null);
const termDraft = ref("");
const termSearch = ref("");
const editingTerm = ref<string | null>(null);
const editingDraft = ref("");
const dictionaryError = ref("");
const dictionaryTool = ref<"add" | "search" | null>(null);

const historyEnabled = computed(() => store.settings?.history.enabled ?? true);
const dictionaryTerms = computed(() => store.settings?.dictionary.terms ?? []);
const filteredTerms = computed(() => {
  const q = termSearch.value.trim().toLocaleLowerCase();
  if (!q) return dictionaryTerms.value;
  return dictionaryTerms.value.filter((term) => term.toLocaleLowerCase().includes(q));
});

// 统计口径（05 §8）
const totalMinutes = computed(() => (stats.value?.total_duration_ms ?? 0) / 60000);
const totalChars = computed(() => stats.value?.total_chars ?? 0);
/// 节省时间 = 打字耗时（打字基准可在设置-历史调整，默认 45 字/分）− 口述时长，负值取 0
const typingWpm = computed(() => store.settings?.history.typing_wpm || 45);
const savedMinutes = computed(() =>
  Math.max(0, totalChars.value / typingWpm.value - totalMinutes.value),
);
const speed = computed(() =>
  totalMinutes.value > 0 ? Math.round(totalChars.value / totalMinutes.value) : 0,
);

function durationParts(minutes: number) {
  const h = Math.floor(minutes / 60);
  const m = Math.round(minutes % 60);
  return { h, m };
}

const totalDurationParts = computed(() => durationParts(totalMinutes.value));
const savedDurationParts = computed(() => durationParts(savedMinutes.value));

function fmtTime(ms: number) {
  const d = new Date(ms);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) {
    return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  }
  const yesterday = new Date(today.getTime() - 86400000);
  if (d.toDateString() === yesterday.toDateString()) return t("home.yesterday");
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

function modeLabel(mode: string): string {
  return te(`modes.${mode}`) ? t(`modes.${mode}`) : mode;
}

async function refresh() {
  const s = await commands.getStats();
  if (s.status === "ok") stats.value = s.data;
  const r = await commands.queryHistory("", 0);
  if (r.status === "ok") {
    recent.value = r.data.slice(0, 3);
    if (tab.value === "history") items.value = r.data;
  }
}

async function doSearch() {
  const r = await commands.queryHistory(search.value, 0);
  if (r.status === "ok") items.value = r.data;
}

async function copyItem(item: HistoryItem) {
  await navigator.clipboard.writeText(item.result);
}

async function deleteItem(item: HistoryItem) {
  await commands.deleteHistoryItem(item.id);
  await refresh();
  await doSearch();
}

async function clearAll() {
  await commands.clearHistory();
  await refresh();
  items.value = [];
}

function normalizeTerm(value: string) {
  return value.trim().slice(0, DICTIONARY_MAX_TERM_CHARS);
}

async function saveTerms(terms: string[]) {
  await store.mutate((d) => {
    d.dictionary.terms = terms;
  });
}

async function addTerm() {
  const term = normalizeTerm(termDraft.value);
  if (!term) {
    dictionaryError.value = t("home.dictionary_empty");
    return;
  }
  if (dictionaryTerms.value.includes(term)) {
    dictionaryError.value = t("home.dictionary_duplicate");
    return;
  }
  if (dictionaryTerms.value.length >= DICTIONARY_MAX_TERMS) {
    dictionaryError.value = t("home.dictionary_limit", { n: DICTIONARY_MAX_TERMS });
    return;
  }
  await saveTerms([...dictionaryTerms.value, term]);
  termDraft.value = "";
  dictionaryError.value = "";
}

function toggleDictionaryTool(tool: "add" | "search") {
  dictionaryTool.value = dictionaryTool.value === tool ? null : tool;
  dictionaryError.value = "";
}

function closeDictionaryTool() {
  if (dictionaryTool.value === "add") termDraft.value = "";
  if (dictionaryTool.value === "search") termSearch.value = "";
  dictionaryTool.value = null;
  dictionaryError.value = "";
}

function startEdit(term: string) {
  editingTerm.value = term;
  editingDraft.value = term;
  dictionaryError.value = "";
}

function cancelEdit() {
  editingTerm.value = null;
  editingDraft.value = "";
}

async function commitEdit() {
  const oldTerm = editingTerm.value;
  const nextTerm = normalizeTerm(editingDraft.value);
  if (!oldTerm) return;
  if (!nextTerm) {
    dictionaryError.value = t("home.dictionary_empty");
    return;
  }
  if (nextTerm !== oldTerm && dictionaryTerms.value.includes(nextTerm)) {
    dictionaryError.value = t("home.dictionary_duplicate");
    return;
  }
  await saveTerms(dictionaryTerms.value.map((term) => (term === oldTerm ? nextTerm : term)));
  cancelEdit();
  dictionaryError.value = "";
}

async function deleteTerm(term: string) {
  await saveTerms(dictionaryTerms.value.filter((item) => item !== term));
  if (editingTerm.value === term) cancelEdit();
  dictionaryError.value = "";
}

function openSettings() {
  commands.openSettingsWindow();
}

function toggleTheme() {
  const cur = store.settings?.general.theme ?? "system";
  const next = cur === "dark" ? "light" : "dark";
  store.mutate((d) => void (d.general.theme = next));
  document.documentElement.setAttribute("data-theme", next);
}

const dictKey = computed(() => {
  const keys = store.settings?.hotkeys.dictation ?? defaultHotkeys.value.dictation;
  return keys.map((key) => keyLabel(key, t, te)).join(" + ");
});
const assistKey = computed(() => {
  const keys = store.settings?.hotkeys.assistant ?? defaultHotkeys.value.assistant;
  return keys.map((key) => keyLabel(key, t, te)).join(" + ");
});

onMounted(async () => {
  await store.load();
  await refresh();
  await events.sessionSnapshotEvent.listen((e) => {
    if (e.payload.phase === "idle") refresh(); // 会话结束刷新统计
  });
});
</script>

<template>
  <div class="home-root">
    <!-- Overlay 标题栏：顶部拖拽区 -->
    <div class="titlebar" data-tauri-drag-region></div>
    <!-- 侧边栏（180px，--surface-2 底） -->
    <aside class="side">
      <div class="brand">
        <span class="mini">
          <span class="g"><i class="m1" /><i class="m2" /><i class="m3" /><i class="m4" /><i class="m5" /></span>
          <span class="s" />
        </span>
        <b>Typex</b>
      </div>
      <nav>
        <div :class="{ on: tab === 'overview' }" @click="tab = 'overview'">
          <svg class="nav-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <path d="M4.5 10.5 12 4l7.5 6.5" />
            <path d="M6.5 9.5v9h11v-9" />
          </svg>
          <span class="nav-label">{{ t("home.nav_overview") }}</span>
        </div>
        <div :class="{ on: tab === 'history' }" @click="tab = 'history'; doSearch()">
          <svg class="nav-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <circle cx="12" cy="12" r="7.5" />
            <path d="M12 7.8v4.6l3.1 1.9" />
          </svg>
          <span class="nav-label">{{ t("home.nav_history") }}</span>
        </div>
        <div :class="{ on: tab === 'dictionary' }" @click="tab = 'dictionary'">
          <svg class="nav-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <path d="M12 6.5v13" />
            <path d="M4.5 5.5h4.2c1.8 0 3.3 1.5 3.3 3.3v10.7c0-1.5-1.2-2.7-2.7-2.7H4.5z" />
            <path d="M19.5 5.5h-4.2c-1.8 0-3.3 1.5-3.3 3.3v10.7c0-1.5 1.2-2.7 2.7-2.7h4.8z" />
          </svg>
          <span class="nav-label">{{ t("home.nav_dictionary") }}</span>
        </div>
      </nav>
      <div class="mfoot">
        <button type="button" :title="t('home.settings')" :aria-label="t('home.settings')" @click="openSettings">⚙</button>
        <button type="button" :title="t('home.toggle_theme')" :aria-label="t('home.toggle_theme')" @click="toggleTheme">◐</button>
      </div>
    </aside>

    <!-- 首页页签 -->
    <main v-if="tab === 'overview'" class="main">
      <div class="hero">
        <h4>{{ t("home.hero_title") }}</h4>
        <p>
          <i18n-t keypath="home.hero_hint" scope="global">
            <template #dict><Kbd>{{ dictKey }}</Kbd></template>
            <template #assist><Kbd>{{ assistKey }}</Kbd></template>
          </i18n-t>
        </p>
      </div>

      <div v-if="historyEnabled" class="stats">
        <div class="stat">
          <b v-if="totalDurationParts.h > 0">
            {{ totalDurationParts.h }}<small>{{ t("home.unit_hour") }}</small>{{ totalDurationParts.m }}<small>{{ t("home.unit_min") }}</small>
          </b>
          <b v-else>{{ totalDurationParts.m }}<small>{{ t("home.unit_min") }}</small></b>
          <span>{{ t("home.stat_duration") }}</span>
        </div>
        <div class="stat"><b>{{ totalChars.toLocaleString() }}<small>{{ t("home.unit_char") }}</small></b><span>{{ t("home.stat_chars") }}</span></div>
        <div class="stat">
          <b v-if="savedDurationParts.h > 0">
            {{ savedDurationParts.h }}<small>{{ t("home.unit_hour") }}</small>{{ savedDurationParts.m }}<small>{{ t("home.unit_min") }}</small>
          </b>
          <b v-else>{{ savedDurationParts.m }}<small>{{ t("home.unit_min") }}</small></b>
          <span>{{ t("home.stat_saved") }}</span>
        </div>
        <div class="stat"><b>{{ speed }}<small>{{ t("home.unit_cpm") }}</small></b><span>{{ t("home.stat_speed") }}</span></div>
      </div>

      <div class="sec-h">
        <h6>{{ t("home.recent") }}</h6>
        <a @click="tab = 'history'; doSearch()">{{ t("home.view_all") }}</a>
      </div>
      <div v-if="recent.length" class="recent">
        <div v-for="item in recent" :key="item.id" class="hrow">
          <time>{{ fmtTime(item.created_at) }}</time>
          <span class="tag">{{ modeLabel(item.mode) }}</span>
          <span v-if="item.app_name" class="app">{{ item.app_name }}</span>
          <span class="sum">{{ item.result }}</span>
        </div>
      </div>
      <div v-else class="empty">
        <div class="glyph">⌀</div>
        {{ t("home.empty") }}<br />
        <span class="empty-hint">{{ t("home.empty_hint", { key: dictKey }) }}</span>
      </div>
    </main>

    <!-- 历史记录页签 -->
    <main v-else-if="tab === 'history'" class="main hist">
      <div class="hist-top">
        <Input v-model="search" :placeholder="t('home.search_ph')" @keydown.enter="doSearch" />
        <Button variant="danger" size="sm" @click="clearAll">{{ t("home.clear_all") }}</Button>
      </div>
      <div class="recent scroll" :class="{ 'scroll-empty': !items.length }">
        <template v-for="item in items" :key="item.id">
          <div class="hrow clickable" @click="expanded = expanded === item.id ? null : item.id">
            <time>{{ fmtTime(item.created_at) }}</time>
            <span class="tag">{{ modeLabel(item.mode) }}</span>
            <span v-if="item.app_name" class="app">{{ item.app_name }}</span>
            <span class="sum">{{ item.result }}</span>
          </div>
          <div v-if="expanded === item.id" class="hexp">
            <div class="cols">
              <div><small>{{ item.mode === "assistant" ? t("home.voice_instruction") : t("home.transcript") }}</small>{{ item.transcript }}</div>
              <div><small>{{ item.mode === "assistant" ? t("home.assistant_result") : item.mode === "translation" ? t("home.result_translated") : t("home.result_polished") }}</small>{{ item.result }}</div>
            </div>
            <div class="hexp-actions">
              <Button size="sm" @click="copyItem(item)">{{ t("actions.copy") }}</Button>
              <Button variant="ghost" size="sm" @click="deleteItem(item)">{{ t("actions.delete") }}</Button>
            </div>
          </div>
        </template>
        <div v-if="!items.length" class="empty hist-empty">
          <div class="glyph">⌀</div>
          {{ t("home.no_match") }}
        </div>
      </div>
    </main>

    <!-- 词典页签 -->
    <main v-else class="main dictionary">
      <div class="dict-head">
        <div class="dict-copy">
          <h4>{{ t("home.dictionary_title") }}</h4>
          <p>{{ t("home.dictionary_hint") }}</p>
        </div>
        <div class="dict-head-side">
          <span>{{ t("home.dictionary_count", { n: dictionaryTerms.length, max: DICTIONARY_MAX_TERMS }) }}</span>
          <div class="dict-tools">
            <div v-if="dictionaryTool === 'add'" class="dict-tool-panel">
              <Input
                v-model="termDraft"
                :placeholder="t('home.dictionary_add_ph')"
                @keydown.enter="addTerm"
                @keydown.esc="closeDictionaryTool"
              />
              <button
                type="button"
                class="dict-icon-btn"
                :title="t('actions.add')"
                :aria-label="t('actions.add')"
                @click="addTerm"
              >✓</button>
            </div>
            <button
              v-else
              type="button"
              class="dict-icon-btn"
              :title="t('actions.add')"
              :aria-label="t('actions.add')"
              @click="toggleDictionaryTool('add')"
            >＋</button>
            <div v-if="dictionaryTool === 'search'" class="dict-tool-panel">
              <Input
                v-model="termSearch"
                :placeholder="t('home.dictionary_search_ph')"
                @keydown.esc="closeDictionaryTool"
              />
              <button
                type="button"
                class="dict-icon-btn"
                :title="t('actions.cancel')"
                :aria-label="t('actions.cancel')"
                @click="closeDictionaryTool"
              >×</button>
            </div>
            <button
              v-else
              type="button"
              class="dict-icon-btn dict-search-btn"
              :title="t('actions.search')"
              :aria-label="t('actions.search')"
              @click="toggleDictionaryTool('search')"
            >⌕</button>
          </div>
        </div>
      </div>
      <p v-if="dictionaryError" class="dict-error">{{ dictionaryError }}</p>
      <div class="dict-list" :class="{ 'dict-list-empty': !filteredTerms.length }">
        <template v-if="filteredTerms.length">
          <div v-for="term in filteredTerms" :key="term" class="term-row">
            <Input
              v-if="editingTerm === term"
              v-model="editingDraft"
              @keydown.enter="commitEdit"
              @keydown.esc="cancelEdit"
            />
            <span v-else>{{ term }}</span>
            <div class="term-actions">
              <template v-if="editingTerm === term">
                <Button size="sm" @click="commitEdit">{{ t("actions.save") }}</Button>
                <Button variant="ghost" size="sm" @click="cancelEdit">{{ t("actions.cancel") }}</Button>
              </template>
              <template v-else>
                <Button variant="ghost" size="sm" @click="startEdit(term)">{{ t("actions.edit") }}</Button>
                <Button variant="ghost" size="sm" @click="deleteTerm(term)">{{ t("actions.delete") }}</Button>
              </template>
            </div>
          </div>
        </template>
        <div v-else class="empty hist-empty">
          <div class="glyph">⌀</div>
          {{ termSearch ? t("home.dictionary_no_match") : t("home.dictionary_empty_state") }}
        </div>
      </div>
    </main>
  </div>
</template>

<style scoped>
.home-root {
  display: flex;
  position: relative;
  width: 100vw;
  height: 100vh;
  background: var(--surface);
  overflow: hidden;
}
.home-root::before {
  content: "";
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 1px;
  background: var(--border-2);
  pointer-events: none;
  z-index: 10;
}
.titlebar {
  display: none;
}
.side {
  width: 180px;
  flex-shrink: 0;
  background: var(--surface-2);
  display: flex;
  flex-direction: column;
  padding: 16px 10px 12px;
}
.brand {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px 16px;
}
/* mini 图标 = 小尺寸 glyph 规则（04 §2.2）；此处用五柱缩微保持与托盘波形一致 */
.mini {
  width: 26px;
  height: 26px;
  border-radius: 7px;
  background: var(--icon-bg, #000);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1.5px;
}
.mini .g {
  display: flex;
  gap: 1.5px;
  align-items: center;
}
.mini .g i {
  width: 2px;
  border-radius: 1px;
  background: var(--icon-fg, #fff);
  display: block;
}
.m1, .m5 { height: 4px; }
.m2, .m4 { height: 7px; }
.m3 { height: 10px; }
.mini .s {
  width: 2px;
  height: 7px;
  background: var(--icon-fg, #fff);
  border-radius: 1px;
}
.brand b {
  font-size: 15px;
  letter-spacing: -0.01em;
}
nav div {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-radius: var(--radius-control);
  color: var(--text-2);
  font-size: 13px;
  margin-bottom: 2px;
  cursor: pointer;
}
nav div:hover {
  background: var(--sel-bg);
}
nav .on {
  background: var(--sel-bg);
  color: var(--text-1);
  font-weight: 600;
}
.nav-icon {
  width: 16px;
  height: 16px;
  flex: 0 0 16px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}
.nav-label {
  line-height: 16px;
}
.mfoot {
  margin-top: auto;
  display: flex;
  gap: 4px;
  padding: 0 6px;
}
.mfoot button {
  width: 28px;
  height: 28px;
  border-radius: 7px;
  border: 0;
  padding: 0;
  background: transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  font-size: 13px;
  font-family: inherit;
  cursor: pointer;
  user-select: none;
  -webkit-user-select: none;
}
.mfoot button:hover {
  background: var(--sel-bg);
  color: var(--text-1);
}
.main {
  flex: 1;
  padding: 26px 28px 16px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.hero h4 {
  font-size: 21px;
  letter-spacing: -0.01em;
  margin-bottom: 8px;
  font-weight: 600;
}
.hero p {
  font-size: 13px;
  color: var(--text-2);
}
.stats {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 12px;
  margin: 22px 0;
}
.stat {
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  padding: 16px 16px 13px;
}
.stat b {
  display: block;
  font-size: 24px;
  font-weight: 600;
  letter-spacing: -0.02em;
}
.stat b small {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-2);
  margin-left: 2px;
}
.stat span {
  font-size: 11.5px;
  color: var(--text-3);
}
.sec-h {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  margin-bottom: 8px;
}
.sec-h h6 {
  font-size: 13px;
  font-weight: 600;
}
.sec-h a {
  font-size: 12px;
  color: var(--text-2);
  cursor: pointer;
}
.recent {
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  overflow: hidden;
}
.recent.scroll {
  overflow-y: auto;
  flex: 1;
}
.recent.scroll-empty {
  border-style: dashed;
  border-color: var(--border-2);
  display: flex;
  align-items: center;
  justify-content: center;
}
.hrow {
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
  font-size: 12.5px;
  display: flex;
  gap: 10px;
  align-items: baseline;
}
.hrow:last-child {
  border-bottom: none;
}
.hrow.clickable {
  cursor: pointer;
}
.hrow.clickable:hover {
  background: var(--surface-2);
}
.hrow time {
  color: var(--text-3);
  font-size: 11px;
  white-space: nowrap;
  min-width: 34px;
}
.tag {
  display: inline-block;
  padding: 3px 10px;
  border-radius: 99px;
  font-size: 11px;
  background: var(--surface-2);
  color: var(--text-2);
  border: 1px solid var(--border);
  white-space: nowrap;
}
.app {
  color: var(--text-3);
  font-size: 11px;
}
.sum {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.hexp {
  background: var(--surface-2);
  border-bottom: 1px solid var(--border);
  padding: 12px 14px;
}
.hexp .cols {
  display: flex;
  gap: 12px;
  font-size: 12px;
  line-height: 1.6;
}
.hexp .cols > div {
  flex: 1;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-control);
  padding: 8px 10px;
  user-select: text;
}
.hexp .cols small {
  display: block;
  color: var(--text-3);
  font-size: 10.5px;
  margin-bottom: 4px;
}
.hexp-actions {
  display: flex;
  gap: 8px;
  margin-top: 8px;
}
.hist-top {
  display: flex;
  gap: 10px;
  align-items: center;
  margin-bottom: 4px;
}
.empty {
  text-align: center;
  padding: 28px;
  color: var(--text-3);
  font-size: 12.5px;
  border: 1px dashed var(--border-2);
  border-radius: var(--radius-card);
}
.empty .glyph {
  font-size: 22px;
  margin-bottom: 6px;
}
.empty-hint {
  font-size: 11px;
}
.hist-empty {
  border: none;
  padding: 0;
}
.dictionary {
  gap: 12px;
}
.dict-head {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 200px;
  align-items: start;
  gap: 16px;
}
.dict-copy {
  min-width: 0;
}
.dict-head h4 {
  font-size: 21px;
  margin-bottom: 6px;
  font-weight: 600;
}
.dict-head p {
  color: var(--text-2);
  font-size: 12.5px;
  line-height: 1.4;
  margin: 0;
}
.dict-head-side {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  justify-content: flex-start;
  gap: 8px;
  width: 200px;
}
.dict-head-side span {
  color: var(--text-3);
  font-size: 12px;
  line-height: 1.4;
  white-space: nowrap;
}
.dict-tools {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  align-items: center;
  min-height: 34px;
}
.dict-tool-panel {
  display: grid;
  grid-template-columns: 112px 34px;
  align-items: center;
  gap: 6px;
}
.dict-tool-panel :deep(.input) {
  background: var(--surface);
}
.dict-icon-btn {
  width: 34px;
  height: 34px;
  flex: 0 0 34px;
  border-radius: 50%;
  border: 1px solid var(--border-2);
  background: var(--surface);
  color: var(--text-1);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  font-family: inherit;
  font-size: 17px;
  line-height: 1;
  cursor: pointer;
}
.dict-icon-btn:hover {
  background: var(--sel-bg);
}
.dict-icon-btn:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: 2px;
}
.dict-search-btn {
  font-size: 22px;
  padding-bottom: 2px;
}
.dict-error {
  margin: -4px 0 0;
  color: var(--error);
  font-size: 12px;
}
.dict-list {
  flex: 1;
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  overflow-y: auto;
}
.dict-list-empty {
  border-style: dashed;
  border-color: var(--border-2);
  display: flex;
  align-items: center;
  justify-content: center;
}
.term-row {
  min-height: 46px;
  padding: 7px 10px 7px 14px;
  border-bottom: 1px solid var(--border);
  display: flex;
  align-items: center;
  gap: 10px;
}
.term-row:last-child {
  border-bottom: none;
}
.term-row:hover {
  background: var(--surface-2);
}
.term-row > span {
  flex: 1;
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.term-row :deep(.input) {
  flex: 1;
  background: var(--surface);
}
.term-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

@media (max-width: 760px) {
  .dict-head {
    grid-template-columns: 1fr;
  }
  .dict-head-side {
    width: 100%;
  }
  .dict-tools {
    width: 100%;
  }
  .dict-tool-panel {
    grid-template-columns: minmax(0, 1fr) 34px;
  }
}
</style>
