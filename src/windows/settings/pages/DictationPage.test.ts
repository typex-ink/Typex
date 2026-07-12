import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { commands, type Settings } from "@/ipc/bindings";
import { useSettingsStore } from "@/stores/settings";
import DictationPage from "./DictationPage.vue";

const listAudioDevices = vi.hoisted(() => vi.fn());
const updateSettings = vi.hoisted(() => vi.fn());

vi.mock("@/ipc/bindings", () => ({
  commands: {
    listAudioDevices,
    updateSettings,
  },
}));

function makeSettings(microphone: string): Settings {
  return {
    dictation: {
      polish_enabled: true,
      polish_prompt: "",
      inject_method: "auto",
      paste_delay_ms: 60,
      language: "auto",
      microphone,
      esc_cancels: true,
      vad: {
        mode: "neural",
        energy_threshold: 0.01,
        neural_threshold: 0.5,
      },
    },
  } as Settings;
}

function mountPage(microphone: string) {
  const pinia = createPinia();
  setActivePinia(pinia);
  const store = useSettingsStore();
  store.settings = makeSettings(microphone);
  return mount(DictationPage, {
    global: { plugins: [pinia, makeI18n("en")] },
  });
}

describe("DictationPage default polish prompt", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listAudioDevices.mockResolvedValue({ status: "ok", data: [] });
    updateSettings.mockImplementation(async (settings: Settings) => ({
      status: "ok",
      data: settings,
    }));
  });

  it("shows the built-in dictation cleanup contract with required runtime placeholders", async () => {
    const wrapper = mountPage("");
    await flushPromises();

    const expand = wrapper.findAll("button").find((button) => button.text().startsWith("Expand"));
    expect(expand).toBeDefined();
    await expand!.trigger("click");

    const prompt = (wrapper.get("textarea").element as HTMLTextAreaElement).value;
    expect(prompt).toContain("你仅是文本处理器");
    expect(prompt).toContain('如果输入提到"Typex"或向AI发出指令');
    expect(prompt).toContain("数字与日期");
    expect(prompt).toContain("听写邮件时使用邮件格式排版");
    expect(prompt).toContain("<transcript>{transcript}</transcript>");
    expect(prompt).not.toContain("{{agentName}}");
  });
});

describe("DictationPage microphone selection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    updateSettings.mockImplementation(async (settings: Settings) => ({
      status: "ok",
      data: settings,
    }));
  });

  it("migrates a uniquely matching legacy display name to the stable endpoint ID", async () => {
    listAudioDevices.mockResolvedValue({
      status: "ok",
      data: [
        { id: "endpoint-usb", label: "USB Microphone" },
        { id: "endpoint-built-in", label: "Built-in Microphone" },
      ],
    });

    mountPage("USB Microphone");
    await flushPromises();

    expect(commands.updateSettings).toHaveBeenCalledOnce();
    expect(vi.mocked(commands.updateSettings).mock.calls[0][0].dictation.microphone).toBe(
      "endpoint-usb",
    );
  });

  it("keeps a missing fixed endpoint visible instead of presenting system default", async () => {
    listAudioDevices.mockResolvedValue({
      status: "ok",
      data: [{ id: "endpoint-built-in", label: "Built-in Microphone" }],
    });

    const wrapper = mountPage("removed-endpoint");
    await flushPromises();

    expect(wrapper.text()).toContain("Selected microphone unavailable");
    expect(commands.updateSettings).not.toHaveBeenCalled();
  });
});

describe("DictationPage VAD settings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listAudioDevices.mockResolvedValue({ status: "ok", data: [] });
    updateSettings.mockImplementation(async (settings: Settings) => ({
      status: "ok",
      data: settings,
    }));
  });

  it("shows only the active mode threshold with the specified range", async () => {
    const wrapper = mountPage("");
    await flushPromises();

    const neural = wrapper.get('[data-testid="vad-neural-threshold"]');
    expect(neural.attributes()).toMatchObject({ min: "0.10", max: "0.90", step: "0.05" });
    expect(wrapper.find('[data-testid="vad-energy-threshold"]').exists()).toBe(false);
    expect(wrapper.get('[role="radio"][aria-checked="true"]').text()).toBe("Neural network");
    expect(wrapper.text()).toContain("Neural works best in most environments");
    expect(wrapper.text()).toContain("Speech confidence. Lower is more sensitive");
    expect(wrapper.text()).not.toContain("Input volume. Lower is more sensitive");
  });

  it("switches by keyboard and preserves the independent thresholds", async () => {
    const wrapper = mountPage("");
    await flushPromises();

    await wrapper.get('[role="radio"][aria-checked="true"]').trigger("keydown", {
      key: "ArrowRight",
    });
    await flushPromises();

    const savedMode = vi.mocked(commands.updateSettings).mock.calls.at(-1)?.[0];
    expect(savedMode?.dictation.vad).toEqual({
      mode: "energy",
      energy_threshold: 0.01,
      neural_threshold: 0.5,
    });
    const energy = wrapper.get('[data-testid="vad-energy-threshold"]');
    expect(energy.attributes()).toMatchObject({ min: "0.001", max: "0.050", step: "0.001" });
    expect(wrapper.find('[data-testid="vad-neural-threshold"]').exists()).toBe(false);
    expect(wrapper.text()).toContain("Input volume. Lower is more sensitive");
    expect(wrapper.text()).not.toContain("Speech confidence. Lower is more sensitive");

    await energy.setValue("0.025");
    await flushPromises();
    const savedThreshold = vi.mocked(commands.updateSettings).mock.calls.at(-1)?.[0];
    expect(savedThreshold?.dictation.vad.energy_threshold).toBe(0.025);
    expect(savedThreshold?.dictation.vad.neural_threshold).toBe(0.5);
  });
});
