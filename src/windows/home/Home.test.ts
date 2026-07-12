import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { commands, type Settings } from "@/ipc/bindings";
import Home from "./Home.vue";

const mockGetSettings = vi.hoisted(() => vi.fn());
const mockUpdateSettings = vi.hoisted(() => vi.fn());
const mockQueryHistory = vi.hoisted(() => vi.fn());
const mockClipboardWrite = vi.hoisted(() => vi.fn(async () => undefined));

vi.mock("@/ipc/bindings", () => ({
  commands: {
    getSettings: mockGetSettings,
    updateSettings: mockUpdateSettings,
    getStats: vi.fn(async () => ({
      status: "ok",
      data: { total_duration_ms: 0, total_chars: 0 },
    })),
    queryHistory: mockQueryHistory,
    deleteHistoryItem: vi.fn(async () => ({ status: "ok", data: null })),
    clearHistory: vi.fn(async () => ({ status: "ok", data: null })),
    openSettingsWindow: vi.fn(async () => ({ status: "ok", data: null })),
  },
  events: {
    settingsChangedEvent: { listen: vi.fn(async () => vi.fn()) },
    sessionSnapshotEvent: { listen: vi.fn(async () => vi.fn()) },
  },
}));

function makeSettings(): Settings {
  return {
    schema_version: 7,
    general: {
      theme: "system",
      language: "zh_cn",
      autostart: false,
      chimes_enabled: true,
      chimes_volume: 0.7,
      proxy_mode: "system",
      proxy_url: "",
      model_download_source: "auto",
      check_updates: true,
      update_channel: "stable",
    },
    dictation: {
      polish_enabled: true,
      polish_prompt: "",
      inject_method: "auto",
      paste_delay_ms: 40,
      language: "auto",
      microphone: "",
      esc_cancels: true,
      vad: {
        mode: "neural",
        energy_threshold: 0.01,
        neural_threshold: 0.5,
      },
    },
    translation: {
      source_language: "中文",
      target_language: "English",
      bidirectional: true,
      translate_prompt: "",
      recent_targets: ["English"],
    },
    assistant: { process_prompt: "", ask_prompt: "" },
    history: { enabled: true, retention_days: 90, typing_wpm: 45 },
    hotkeys: {
      dictation: ["MetaRight"],
      assistant: ["AltRight"],
      translation: ["MetaRight", "AltRight"],
      hold_threshold_ms: 350,
    },
    dictionary: { terms: [] },
    slots: {},
    profiles: [],
    onboarding_done: true,
  } as Settings;
}

async function mountHome(settings = makeSettings()) {
  const pinia = createPinia();
  setActivePinia(pinia);
  mockGetSettings.mockResolvedValue(settings);
  mockUpdateSettings.mockImplementation(async (next: Settings) => ({
    status: "ok",
    data: next,
  }));
  const wrapper = mount(Home, {
    global: { plugins: [pinia, makeI18n("zh-CN")] },
  });
  await flushPromises();
  return wrapper;
}

describe("Home dictionary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockQueryHistory.mockResolvedValue({ status: "ok", data: [] });
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: mockClipboardWrite },
    });
  });

  it("adds a trimmed dictionary term through settings", async () => {
    const wrapper = await mountHome();
    const dictionaryNav = wrapper.findAll("nav div").find((item) => item.text().includes("词典"));
    expect(dictionaryNav).toBeTruthy();
    await dictionaryNav!.trigger("click");

    const addToggle = wrapper.find("button[aria-label='添加']");
    expect(addToggle.exists()).toBe(true);
    await addToggle.trigger("click");

    const addInput = wrapper.find("input[placeholder='输入高频词…']");
    await addInput.setValue(" Typex ");
    const addButton = wrapper.find(".dict-tool-panel button[aria-label='添加']");
    expect(addButton).toBeTruthy();
    await addButton.trigger("click");

    expect(commands.updateSettings).toHaveBeenCalledOnce();
    const next = vi.mocked(commands.updateSettings).mock.calls[0][0] as Settings;
    expect(next.dictionary.terms).toEqual(["Typex"]);
  });

  it("labels assistant history and copies the complete answer", async () => {
    const assistantItem = {
      id: 7,
      created_at: Date.now(),
      mode: "assistant",
      transcript: "Why did it time out?",
      result: "The upstream service did not answer before the deadline.",
      app_name: "VS Code",
      duration_ms: 2200,
      char_count: 57,
    };
    mockQueryHistory.mockResolvedValue({ status: "ok", data: [assistantItem] });
    const wrapper = await mountHome();
    const historyNav = wrapper.findAll("nav div").find((item) => item.text().includes("历史"));
    await historyNav!.trigger("click");
    await flushPromises();
    await wrapper.get(".hrow.clickable").trigger("click");

    expect(wrapper.get(".hexp").text()).toContain("语音指令");
    expect(wrapper.get(".hexp").text()).toContain("助手结果");
    const copy = wrapper
      .findAll(".hexp button")
      .find((button) => button.text().includes("复制"));
    await copy!.trigger("click");
    expect(mockClipboardWrite).toHaveBeenCalledWith(assistantItem.result);
  });
});
