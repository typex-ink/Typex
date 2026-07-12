import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { commands, type Settings } from "@/ipc/bindings";
import HotkeyRecorder from "@/components/HotkeyRecorder.vue";
import Onboarding from "./Onboarding.vue";

vi.mock("@/ipc/bindings", () => ({
  commands: {
    getSettings: vi.fn(),
    updateSettings: vi.fn(async (newSettings: Settings) => ({
      status: "ok",
      data: newSettings,
    })),
    getPermissionStatus: vi.fn(async () => []),
    openPermissionSettings: vi.fn(),
    getHardwareTier: vi.fn(async () => null),
    listLocalModels: vi.fn(async () => ({ status: "ok", data: [] })),
    upsertProfile: vi.fn(async () => ({ status: "ok" })),
    setProfileSecret: vi.fn(async () => ({ status: "ok" })),
    activateProfile: vi.fn(async () => ({ status: "ok" })),
    downloadLocalModel: vi.fn(async () => ({ status: "ok" })),
    cancelLocalDownload: vi.fn(async () => ({ status: "ok" })),
    completeOnboarding: vi.fn(async () => ({ status: "ok", data: null })),
  },
  events: {
    settingsChangedEvent: { listen: vi.fn(async () => vi.fn()) },
    sessionSnapshotEvent: { listen: vi.fn(async () => vi.fn()) },
    localDownloadProgressEvent: { listen: vi.fn(async () => vi.fn()) },
  },
}));

function makeSettings(): Settings {
  return {
    onboarding_done: false,
    general: {
      language: "zh_cn",
      autostart: false,
    },
    hotkeys: {
      dictation: ["ControlRight"],
      assistant: ["ShiftRight"],
      translation: ["ControlRight", "ShiftRight"],
      hold_threshold_ms: 350,
    },
  } as Settings;
}

async function mountOnboarding() {
  const pinia = createPinia();
  setActivePinia(pinia);
  vi.mocked(commands.getSettings).mockResolvedValue(makeSettings());
  const wrapper = mount(Onboarding, {
    global: { plugins: [pinia, makeI18n("zh-CN")] },
  });
  await flushPromises();
  return wrapper;
}

function buttonByText(wrapper: ReturnType<typeof mount>, text: string) {
  const button = wrapper.findAll("button").find((item) => item.text() === text);
  expect(button, `button ${text}`).toBeTruthy();
  return button!;
}

function keyboard(type: "keydown" | "keyup", code: string) {
  window.dispatchEvent(new KeyboardEvent(type, { code, bubbles: true }));
}

async function goToHotkeys(wrapper: Awaited<ReturnType<typeof mountOnboarding>>) {
  await buttonByText(wrapper, "开始 →").trigger("click");
  await buttonByText(wrapper, "继续 →").trigger("click");
  await buttonByText(wrapper, "继续 →").trigger("click");
}

async function goToFinish(wrapper: Awaited<ReturnType<typeof mountOnboarding>>) {
  await goToHotkeys(wrapper);
  await buttonByText(wrapper, "继续 →").trigger("click");
}

describe("Onboarding", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("第 2 步以后可以回到上一步", async () => {
    const wrapper = await mountOnboarding();

    await buttonByText(wrapper, "开始 →").trigger("click");
    expect(wrapper.text()).toContain("← 上一步");

    await buttonByText(wrapper, "← 上一步").trigger("click");
    expect(wrapper.text()).toContain("开始 →");
    expect(wrapper.text()).not.toContain("← 上一步");
  });

  it("先保存完成设置，再调用后端切换到主页", async () => {
    const wrapper = await mountOnboarding();

    await goToFinish(wrapper);
    await buttonByText(wrapper, "完成").trigger("click");
    await flushPromises();

    const saved = vi.mocked(commands.updateSettings).mock.calls[0][0];
    expect(saved.onboarding_done).toBe(true);
    expect(saved.general.autostart).toBe(true);
    expect(commands.completeOnboarding).toHaveBeenCalledOnce();
    expect(vi.mocked(commands.updateSettings).mock.invocationCallOrder[0])
      .toBeLessThan(vi.mocked(commands.completeOnboarding).mock.invocationCallOrder[0]);
  });

  it("主页切换失败时保留完成页并允许重试", async () => {
    vi.mocked(commands.completeOnboarding).mockResolvedValueOnce({
      status: "error",
      error: { code: "internal", message: "home window failed" },
    });
    const wrapper = await mountOnboarding();
    await goToFinish(wrapper);

    await buttonByText(wrapper, "完成").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain("无法打开 Typex，请重试。");
    expect(buttonByText(wrapper, "完成").attributes("disabled")).toBeUndefined();
  });

  it("设置保存失败时不调用窗口切换命令", async () => {
    vi.mocked(commands.updateSettings).mockResolvedValueOnce({
      status: "error",
      error: { code: "internal", message: "settings write failed" },
    });
    const wrapper = await mountOnboarding();
    await goToFinish(wrapper);

    await buttonByText(wrapper, "完成").trigger("click");
    await flushPromises();

    expect(commands.completeOnboarding).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("无法打开 Typex，请重试。");
  });

  it("提交期间锁定完成按钮并忽略重复点击", async () => {
    let releaseUpdate!: () => void;
    vi.mocked(commands.updateSettings).mockImplementationOnce(
      (settings) => new Promise((resolve) => {
        releaseUpdate = () => resolve({ status: "ok", data: settings });
      }),
    );
    const wrapper = await mountOnboarding();
    await goToFinish(wrapper);
    const finishButton = buttonByText(wrapper, "完成");

    await finishButton.trigger("click");
    expect(finishButton.text()).toBe("正在打开…");
    expect(finishButton.attributes("disabled")).toBeDefined();
    await finishButton.trigger("click");
    expect(commands.updateSettings).toHaveBeenCalledOnce();

    releaseUpdate();
    await flushPromises();
    expect(commands.completeOnboarding).toHaveBeenCalledOnce();
  });

  it("第 4 步显示三个录制器并独立保存完整组合", async () => {
    const wrapper = await mountOnboarding();
    await goToHotkeys(wrapper);

    const recorders = wrapper.findAllComponents(HotkeyRecorder);
    expect(recorders).toHaveLength(3);
    await recorders[0].get("button").trigger("click");
    keyboard("keydown", "ControlRight");
    keyboard("keydown", "Digit1");
    keyboard("keyup", "Digit1");
    await flushPromises();

    const saved = vi.mocked(commands.updateSettings).mock.calls.at(-1)?.[0];
    expect(saved?.hotkeys.dictation).toEqual(["ControlRight", "Digit1"]);
    expect(saved?.hotkeys.assistant).toEqual(["ShiftRight"]);
    expect(saved?.hotkeys.translation).toEqual(["ControlRight", "ShiftRight"]);
    expect(wrapper.text()).toContain("按住 右 Ctrl + 1 说");

    await recorders[2].get("button").trigger("click");
    keyboard("keydown", "F13");
    keyboard("keydown", "ContextMenu");
    keyboard("keyup", "ContextMenu");
    await flushPromises();

    const translated = vi.mocked(commands.updateSettings).mock.calls.at(-1)?.[0];
    expect(translated?.hotkeys.translation).toEqual(["F13", "Menu"]);
  });

  it("第 4 步拒绝与另一快捷键相同的组合", async () => {
    const wrapper = await mountOnboarding();
    await goToHotkeys(wrapper);

    const recorders = wrapper.findAllComponents(HotkeyRecorder);
    await recorders[0].get("button").trigger("click");
    keyboard("keydown", "ShiftRight");
    keyboard("keyup", "ShiftRight");
    await flushPromises();

    expect(commands.updateSettings).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("听写与助手不能相同或互相包含");
    expect(wrapper.text()).toContain("右 Ctrl");
  });
});
