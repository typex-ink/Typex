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
  events: {
    audioLevelEvent: { listen: vi.fn(async () => () => {}) },
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
