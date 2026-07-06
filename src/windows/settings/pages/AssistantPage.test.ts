import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { commands, type Settings } from "@/ipc/bindings";
import { useSettingsStore } from "@/stores/settings";
import AssistantPage from "./AssistantPage.vue";

vi.mock("@/ipc/bindings", () => ({
  commands: {
    getSettings: vi.fn(),
    updateSettings: vi.fn(async (newSettings: Settings) => ({
      status: "ok",
      data: newSettings,
    })),
  },
  events: {
    settingsChangedEvent: { listen: vi.fn() },
  },
}));

function makeSettings(): Settings {
  return {
    assistant: { process_prompt: "", ask_prompt: "" },
  } as Settings;
}

function mountPage() {
  const pinia = createPinia();
  setActivePinia(pinia);
  const store = useSettingsStore();
  store.settings = makeSettings();
  return mount(AssistantPage, {
    global: { plugins: [pinia, makeI18n("zh-CN")] },
  });
}

describe("AssistantPage", () => {
  beforeEach(() => vi.clearAllMocks());

  it("保存选区处理提示词到 process_prompt", async () => {
    const wrapper = mountPage();

    expect(wrapper.text()).toContain("选区处理提示词");
    await wrapper.findAll("button")[0].trigger("click");
    await wrapper.find("textarea").setValue("处理 {selection}：{instruction}");
    await wrapper.findAll("button").find((button) => button.text() === "保存")!.trigger("click");

    const saved = vi.mocked(commands.updateSettings).mock.calls[0][0];
    expect(saved.assistant.process_prompt).toBe("处理 {selection}：{instruction}");
    expect(saved.assistant.ask_prompt).toBe("");
  });

  it("展开一个提示词时保留另一个提示词入口", async () => {
    const wrapper = mountPage();

    await wrapper.findAll("button")[0].trigger("click");

    expect(wrapper.findAll("textarea")).toHaveLength(1);
    expect(wrapper.text()).toContain("选区处理提示词");
    expect(wrapper.text()).toContain("问答提示词");
    expect(wrapper.findAll("button").some((button) => button.text().includes("展开"))).toBe(true);
  });

  it("保存问答提示词到 ask_prompt", async () => {
    const wrapper = mountPage();

    expect(wrapper.text()).toContain("问答提示词");
    await wrapper.findAll("button")[1].trigger("click");
    await wrapper.find("textarea").setValue("回答：{instruction}");
    await wrapper.findAll("button").find((button) => button.text() === "保存")!.trigger("click");

    const saved = vi.mocked(commands.updateSettings).mock.calls[0][0];
    expect(saved.assistant.process_prompt).toBe("");
    expect(saved.assistant.ask_prompt).toBe("回答：{instruction}");
  });
});
