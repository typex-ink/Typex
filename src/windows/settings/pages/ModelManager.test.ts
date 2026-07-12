import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { commands, type LocalModelInfo, type Settings } from "@/ipc/bindings";
import ModelManager from "./ModelManager.vue";

const getSettings = vi.hoisted(() => vi.fn());
const listLocalModels = vi.hoisted(() => vi.fn());
const downloadLocalModel = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

vi.mock("@/ipc/bindings", () => ({
  commands: {
    getSettings,
    updateSettings: vi.fn(),
    listLocalModels,
    getHardwareTier: vi.fn(async () => ({
      ram_gb: 8,
      cores: 4,
      gpu: false,
      gpu_backend: "none",
      tier: "standard",
      summary: "RAM 8 GB",
    })),
    downloadLocalModel,
    cancelLocalDownload: vi.fn(async () => ({ status: "ok", data: null })),
    deleteLocalModel: vi.fn(async () => ({ status: "ok", data: null })),
    importLocalModel: vi.fn(async () => ({ status: "ok", data: null })),
  },
  events: {
    settingsChangedEvent: { listen: vi.fn(async () => vi.fn()) },
    localDownloadProgressEvent: { listen: vi.fn(async () => vi.fn()) },
  },
}));

function makeSettings(): Settings {
  return {
    general: { model_download_source: "auto" },
    slots: {},
    profiles: [],
  } as unknown as Settings;
}

function makeModel(id: string, downloadable: boolean): LocalModelInfo {
  return {
    id,
    display_name: id === "large" ? "Large model" : "No-source model",
    purpose: "llm",
    engine: "llama",
    bytes: 1_000_000,
    downloaded: false,
    downloading: false,
    min_ram_gb: 32,
    requires_gpu: true,
    hardware_ok: false,
    tier: "",
    origin: "builtin",
    license: "Apache-2.0",
    downloadable,
    source_names: downloadable ? ["HuggingFace"] : [],
    notes: "",
  };
}

async function mountPage() {
  const pinia = createPinia();
  setActivePinia(pinia);
  const wrapper = mount(ModelManager, {
    global: { plugins: [pinia, makeI18n("zh-CN")] },
  });
  await flushPromises();
  return wrapper;
}

describe("ModelManager download eligibility", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getSettings.mockResolvedValue(makeSettings());
    listLocalModels.mockResolvedValue({
      status: "ok",
      data: [makeModel("large", true), makeModel("imported", false)],
    });
    downloadLocalModel.mockResolvedValue({ status: "ok", data: null });
  });

  it("allows a remote model download when hardware is below recommendation", async () => {
    const wrapper = await mountPage();
    const row = wrapper.findAll(".prov").find((item) => item.text().includes("Large model"))!;
    const button = row.get("button");

    expect(row.text()).toContain("低于建议");
    expect(button.attributes("disabled")).toBeUndefined();
    await button.trigger("click");

    expect(commands.downloadLocalModel).toHaveBeenCalledWith("large", "auto");
  });

  it("keeps a model without a remote source disabled", async () => {
    const wrapper = await mountPage();
    const row = wrapper.findAll(".prov").find((item) => item.text().includes("No-source model"))!;

    expect(row.get("button").attributes("disabled")).toBeDefined();
    expect(commands.downloadLocalModel).not.toHaveBeenCalled();
  });
});
