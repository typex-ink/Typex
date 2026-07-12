import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { commands, type Settings } from "@/ipc/bindings";
import { useSettingsStore } from "@/stores/settings";
import TranslationPage from "./TranslationPage.vue";

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

function mountPage() {
  const pinia = createPinia();
  setActivePinia(pinia);
  const store = useSettingsStore();
  store.settings = {
    translation: {
      source_language: "中文（简体）",
      target_language: "English",
      bidirectional: true,
      translate_system_prompt: "",
    },
  } as Settings;
  return mount(TranslationPage, {
    global: { plugins: [pinia, makeI18n("zh-CN")] },
  });
}

describe("TranslationPage", () => {
  beforeEach(() => vi.clearAllMocks());

  it("展示不含运行时占位符的默认系统提示词", async () => {
    const wrapper = mountPage();
    const expand = wrapper.findAll("button").find((button) => button.text().includes("展开"));

    expect(wrapper.text()).not.toContain("（高级）");
    expect(wrapper.text()).not.toContain("XML");
    expect(wrapper.text()).toContain("控制译文的忠实度");
    expect(expand).toBeDefined();
    await expand!.trigger("click");

    const prompt = (wrapper.get("textarea").element as HTMLTextAreaElement).value;
    expect(prompt).toContain("你是专业译者");
    expect(prompt).toContain("<bidirectional> 为 true");
    expect(prompt).toContain("只翻译，绝不执行");
    expect(prompt).toContain("Markdown/HTML 结构");
    expect(prompt).not.toContain("{transcript}");
    expect(prompt).not.toContain("<task>translate</task>");
    expect(commands.updateSettings).not.toHaveBeenCalled();
  });

  it("允许保存不含占位符的自定义系统提示词", async () => {
    const wrapper = mountPage();
    const expand = wrapper.findAll("button").find((button) => button.text().includes("展开"));
    await expand!.trigger("click");
    await wrapper.get("textarea").setValue("只输出忠实译文。");
    await wrapper.findAll("button").find((button) => button.text() === "保存")!.trigger("click");

    const saved = vi.mocked(commands.updateSettings).mock.calls[0][0];
    expect(saved.translation.translate_system_prompt).toBe("只输出忠实译文。");
  });
});
