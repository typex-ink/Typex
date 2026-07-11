import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { commands, type Settings } from "@/ipc/bindings";
import HotkeyRecorder from "@/components/HotkeyRecorder.vue";
import Onboarding from "./Onboarding.vue";

const closeWindow = vi.hoisted(() => vi.fn(async () => {}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ close: closeWindow }),
}));

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

describe("Onboarding", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    closeWindow.mockClear();
  });

  it("第 2 步以后可以回到上一步", async () => {
    const wrapper = await mountOnboarding();

    await buttonByText(wrapper, "开始 →").trigger("click");
    expect(wrapper.text()).toContain("← 上一步");

    await buttonByText(wrapper, "← 上一步").trigger("click");
    expect(wrapper.text()).toContain("开始 →");
    expect(wrapper.text()).not.toContain("← 上一步");
  });

  it("完成时保存 onboarding_done 并关闭引导窗口", async () => {
    const wrapper = await mountOnboarding();

    await buttonByText(wrapper, "开始 →").trigger("click");
    await buttonByText(wrapper, "继续 →").trigger("click");
    await buttonByText(wrapper, "继续 →").trigger("click");
    await buttonByText(wrapper, "继续 →").trigger("click");
    await buttonByText(wrapper, "完成").trigger("click");
    await flushPromises();

    const saved = vi.mocked(commands.updateSettings).mock.calls[0][0];
    expect(saved.onboarding_done).toBe(true);
    expect(saved.general.autostart).toBe(true);
    expect(closeWindow).toHaveBeenCalledOnce();
  });

  it("第 4 步可修改快捷键并同步保存翻译组合", async () => {
    const wrapper = await mountOnboarding();
    await goToHotkeys(wrapper);

    const recorders = wrapper.findAllComponents(HotkeyRecorder);
    expect(recorders).toHaveLength(2);
    await recorders[0].get("button").trigger("click");
    keyboard("keydown", "ControlRight");
    keyboard("keydown", "Digit1");
    keyboard("keyup", "Digit1");
    await flushPromises();

    const saved = vi.mocked(commands.updateSettings).mock.calls.at(-1)?.[0];
    expect(saved?.hotkeys.dictation).toEqual(["ControlRight", "Digit1"]);
    expect(saved?.hotkeys.assistant).toEqual(["ShiftRight"]);
    expect(saved?.hotkeys.translation).toEqual(["ControlRight", "Digit1", "ShiftRight"]);
    expect(wrapper.text()).toContain("右 Ctrl + 1 + 右 Shift");
    expect(wrapper.text()).toContain("按住 右 Ctrl + 1 说");
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
    expect(wrapper.text()).toContain("任一组合都不能包含另一组合");
    expect(wrapper.text()).toContain("右 Ctrl");
  });
});
