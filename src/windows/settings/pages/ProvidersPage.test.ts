import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { commands, type Settings } from "@/ipc/bindings";
import { useSettingsStore } from "@/stores/settings";
import ProvidersPage from "./ProvidersPage.vue";

const mockGetSettings = vi.hoisted(() => vi.fn());

vi.mock("@/ipc/bindings", () => ({
  commands: {
    getSettings: mockGetSettings,
    activateProfile: vi.fn(async () => ({ status: "ok" })),
    listLocalModels: vi.fn(async () => ({ status: "ok", data: [] })),
  },
  events: {
    settingsChangedEvent: { listen: vi.fn() },
  },
}));

function makeSettings(): Settings {
  return {
    slots: {
      stt: { active_profile: "stt-groq" },
      polish: { active_profile: "llm-deepseek" },
      translate: { active_profile: "llm-deepseek" },
      assistant: { active_profile: "llm-claude" },
    },
    profiles: [
      {
        id: "stt-groq",
        capability: "stt",
        kind: "openai_compat",
        label: "Groq STT",
        base_url: "https://api.groq.com/openai/v1",
        model: "whisper-large-v3-turbo",
        credentials: {},
        extra_headers: {},
        extra_form: {},
        timeout_ms: 30000,
        options: {},
      },
      {
        id: "llm-deepseek",
        capability: "llm",
        kind: "chat_completions",
        label: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        model: "deepseek-chat",
        credentials: {},
        extra_headers: {},
        extra_form: {},
        timeout_ms: 30000,
        options: {},
      },
      {
        id: "llm-claude",
        capability: "llm",
        kind: "responses",
        label: "Claude",
        base_url: "https://api.example.com/v1",
        model: "claude-fable-5",
        credentials: {},
        extra_headers: {},
        extra_form: {},
        timeout_ms: 30000,
        options: {},
      },
    ],
  } as Settings;
}

function mountPage(settings = makeSettings()) {
  const pinia = createPinia();
  setActivePinia(pinia);
  const store = useSettingsStore();
  store.settings = settings;
  return mount(ProvidersPage, {
    global: { plugins: [pinia, makeI18n("zh-CN")] },
  });
}

describe("ProvidersPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetSettings.mockResolvedValue(makeSettings());
  });

  it("LLM 功能从同一个配置池切换，且不混入 STT 服务", async () => {
    const wrapper = mountPage();

    const switchButtons = wrapper
      .findAll("button")
      .filter((button) => button.text().includes("切换"));
    await switchButtons[3].trigger("click");

    const menu = wrapper.find(".menu");
    expect(menu.text()).toContain("DeepSeek");
    expect(menu.text()).toContain("Claude");
    expect(menu.text()).not.toContain("Groq STT");

    const deepseek = menu
      .findAll(".it")
      .find((item) => item.text().includes("DeepSeek"))!;
    await deepseek.trigger("click");

    expect(commands.activateProfile).toHaveBeenCalledWith("assistant", "llm-deepseek");
  });

  it("未配置功能点击配置时先选择已有兼容服务", async () => {
    const settings = makeSettings();
    settings.slots.assistant = { active_profile: null };
    const wrapper = mountPage(settings);

    const configure = wrapper
      .findAll("button")
      .find((button) => button.text().includes("配置"))!;
    await configure.trigger("click");

    const menu = wrapper.find(".menu");
    expect(menu.text()).toContain("DeepSeek");
    expect(menu.text()).toContain("Claude");
    expect(menu.text()).not.toContain("Groq STT");

    const claude = menu
      .findAll(".it")
      .find((item) => item.text().includes("Claude"))!;
    await claude.trigger("click");

    expect(commands.activateProfile).toHaveBeenCalledWith("assistant", "llm-claude");
  });
});
