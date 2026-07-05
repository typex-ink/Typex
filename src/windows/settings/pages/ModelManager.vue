<script setup lang="ts">
// 已下载模型管理子页（05 §5.1 / mockup 2.9 / CP-8.7）：
// 已下载列表（体积/被哪些槽使用/删除警告）+ 可下载列表（硬件要求 + 本机检测 ✓/✗）+ 占用合计。
// 下载源切换（HF/ModelScope/自动）：settings schema 暂无 download_source 字段，
// 先不做源切换 UI（03 §8 双源自动择优是后端行为）；schema 加字段后补（mockup 2.9 底部「更改…」）。
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import Button from "@/components/Button.vue";
import { useSettingsStore } from "@/stores/settings";
import { formatBytes } from "@/shared/format";
import {
  commands,
  events,
  type HardwareTier,
  type LocalModelInfo,
  type SlotKind,
} from "@/ipc/bindings";

const emit = defineEmits<{ back: [] }>();
const { t } = useI18n();
const store = useSettingsStore();

const models = ref<LocalModelInfo[]>([]);
const hw = ref<HardwareTier | null>(null);
/** model_id → 下载进度 0–100 */
const progress = ref<Record<string, number>>({});
/** 删除警告中的模型 id（被槽位引用时先警告再 force） */
const confirmDelete = ref<string | null>(null);
let unlisten: (() => void) | null = null;

const downloaded = computed(() => models.value.filter((m) => m.downloaded));
const available = computed(() => models.value.filter((m) => !m.downloaded));
const totalBytes = computed(() =>
  downloaded.value.reduce((sum, m) => sum + m.bytes, 0),
);

const SLOT_LABEL_KEY: Record<SlotKind, string> = {
  stt: "settings.providers.slot_stt",
  polish: "settings.providers.slot_polish",
  translate: "settings.providers.slot_translate",
  assistant: "settings.providers.slot_assistant",
};

/** 模型被哪些槽位使用（active local 档案的 model 指向它） */
function usedBySlots(modelId: string): string[] {
  const s = store.settings;
  if (!s) return [];
  const out: string[] = [];
  for (const slot of Object.keys(s.slots) as SlotKind[]) {
    const pid = s.slots[slot]?.active_profile;
    const p = s.profiles.find((x) => x.id === pid);
    if (p && p.kind === "local" && p.model === modelId) out.push(t(SLOT_LABEL_KEY[slot]));
  }
  return out;
}

/** 行内硬件要求 + 本机检测结果（mockup 2.9：「需 GPU 加速（本机 ✓ Metal）」） */
function hardwareLine(m: LocalModelInfo): string {
  const parts: string[] = [];
  if (m.requires_gpu) {
    parts.push(
      t("settings.models.req_gpu", {
        check: hw.value?.gpu ? t("settings.models.check_ok_gpu") : t("settings.models.check_fail"),
      }),
    );
  }
  parts.push(
    t("settings.models.req_ram", {
      min: m.min_ram_gb,
      check:
        hw.value && hw.value.ram_gb >= m.min_ram_gb
          ? t("settings.models.check_ok_ram", { ram: hw.value.ram_gb })
          : t("settings.models.check_fail"),
    }),
  );
  return parts.join(" · ");
}

async function load() {
  const r = await commands.listLocalModels();
  if (r.status === "ok") models.value = r.data;
  hw.value = await commands.getHardwareTier();
}

async function download(id: string) {
  progress.value = { ...progress.value, [id]: 0 };
  await commands.downloadLocalModel(id);
}

async function cancelDownload(id: string) {
  await commands.cancelLocalDownload(id);
  const { [id]: _, ...rest } = progress.value;
  progress.value = rest;
  await load();
}

async function remove(m: LocalModelInfo) {
  const used = usedBySlots(m.id).length > 0;
  if (used && confirmDelete.value !== m.id) {
    confirmDelete.value = m.id; // 被引用：先警告，再点一次 force 删除
    return;
  }
  const r = await commands.deleteLocalModel(m.id, used);
  confirmDelete.value = null;
  if (r.status === "ok") await load();
}

onMounted(async () => {
  await store.load();
  await load();
  unlisten = await events.localDownloadProgressEvent.listen((e) => {
    const p = e.payload;
    if (p.done) {
      const { [p.model_id]: _, ...rest } = progress.value;
      progress.value = rest;
      void load();
    } else if (p.bytes_total > 0) {
      progress.value = {
        ...progress.value,
        [p.model_id]: Math.round((p.bytes_done / p.bytes_total) * 100),
      };
    }
  });
});
onUnmounted(() => unlisten?.());
</script>

<template>
  <div>
    <p class="back"><a @click="emit('back')">← {{ t("settings.nav_providers") }}</a></p>
    <h5 class="page-title">{{ t("settings.models.title") }}</h5>
    <p class="desc">{{ t("settings.models.desc") }}</p>

    <!-- 已下载 -->
    <div v-for="m in downloaded" :key="m.id" class="prov">
      <div class="logo">◉</div>
      <div class="meta">
        <b>{{ m.display_name }}</b>
        <span class="tag">{{ m.purpose === "stt" ? "STT" : "LLM" }}</span><br />
        <small>
          {{ formatBytes(m.bytes) }}
          <template v-if="usedBySlots(m.id).length">
            · {{ t("settings.models.in_use", { slots: usedBySlots(m.id).join(" · ") }) }}
          </template>
        </small>
        <small v-if="confirmDelete === m.id" class="warn">{{ t("settings.models.delete_warning") }}</small>
      </div>
      <Button variant="danger" size="sm" @click="remove(m)">
        {{ confirmDelete === m.id ? t("settings.models.delete_confirm") : t("actions.delete") }}
      </Button>
    </div>
    <p v-if="!downloaded.length" class="empty">{{ t("settings.models.none_downloaded") }}</p>

    <!-- 可下载（模型库） -->
    <div class="slot-h"><span>{{ t("settings.models.available") }}</span></div>
    <div v-for="m in available" :key="m.id" class="prov">
      <div class="logo">◉</div>
      <div class="meta">
        <b>{{ m.display_name }}</b>
        <span class="tag">{{ m.purpose === "stt" ? "STT" : "LLM" }}</span><br />
        <small>{{ formatBytes(m.bytes) }} · {{ hardwareLine(m) }}</small>
        <span v-if="progress[m.id] !== undefined" class="pbar"><i :style="{ width: `${progress[m.id]}%` }" /></span>
      </div>
      <Button v-if="progress[m.id] !== undefined" size="sm" @click="cancelDownload(m.id)">
        {{ t("actions.cancel") }}
      </Button>
      <Button v-else size="sm" :disabled="!m.hardware_ok" @click="download(m.id)">
        {{ t("settings.models.download") }}
      </Button>
    </div>

    <!-- 占用合计 -->
    <p class="total">{{ t("settings.models.total", { size: formatBytes(totalBytes) }) }}</p>
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
  margin-bottom: 6px;
  font-weight: 600;
}
.desc {
  font-size: 12px;
  color: var(--text-2);
  margin-bottom: 12px;
}
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
  display: inline-block;
}
.meta small.warn {
  display: block;
  color: var(--error);
}
.tag {
  font-size: 10px;
  border: 1px solid var(--border-2);
  border-radius: 4px;
  padding: 0 5px;
  margin-left: 4px;
  color: var(--text-2);
}
/* 进度条 = --text-1 实底，无彩色 */
.pbar {
  display: block;
  width: 100%;
  height: 4px;
  border-radius: 99px;
  background: var(--border);
  overflow: hidden;
  margin-top: 7px;
}
.pbar i {
  display: block;
  height: 100%;
  background: var(--text-1);
  transition: width 0.3s;
}
.slot-h {
  font-size: 11px;
  color: var(--text-3);
  letter-spacing: 0.06em;
  margin: 16px 0 8px;
}
.empty {
  font-size: 12px;
  color: var(--text-3);
}
.total {
  font-size: 11px;
  color: var(--text-3);
  margin-top: 12px;
}
</style>
