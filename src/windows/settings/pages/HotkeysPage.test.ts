import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";
import HotkeyRecorder from "@/components/HotkeyRecorder.vue";
import { makeI18n } from "@/i18n";
import { commands, type Settings } from "@/ipc/bindings";
import { useSettingsStore } from "@/stores/settings";
import HotkeysPage from "./HotkeysPage.vue";

const updateSettings = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: () => "windows",
}));

vi.mock("@/ipc/bindings", () => ({
  commands: {
    updateSettings,
  },
  events: {
    settingsChangedEvent: { listen: vi.fn() },
  },
}));

function makeSettings(
  dictation: string[] = ["ControlRight"],
  assistant: string[] = ["AltRight"],
): Settings {
  return {
    schema_version: 7,
    hotkeys: {
      dictation,
      assistant,
      translation: [...dictation, ...assistant],
      hold_threshold_ms: 350,
    },
  } as Settings;
}

function mountPage(settings = makeSettings()) {
  const pinia = createPinia();
  setActivePinia(pinia);
  const store = useSettingsStore();
  store.settings = settings;
  return mount(HotkeysPage, {
    global: { plugins: [pinia, makeI18n("en")] },
  });
}

describe("HotkeysPage chord validation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    updateSettings.mockImplementation(async (settings: Settings) => ({
      status: "ok",
      data: settings,
    }));
  });

  it("blocks equal and subset chords before calling the settings command", async () => {
    const wrapper = mountPage();
    const recorders = wrapper.findAllComponents(HotkeyRecorder);

    recorders[0].vm.$emit("update:modelValue", ["AltRight"]);
    await nextTick();
    expect(commands.updateSettings).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("neither may contain the other");

    recorders[1].vm.$emit("update:modelValue", ["ControlRight", "KeyA"]);
    await nextTick();
    expect(commands.updateSettings).not.toHaveBeenCalled();
  });

  it("saves shared non-subset chords and derives their ordered union", async () => {
    const wrapper = mountPage(
      makeSettings(
        ["ControlRight", "KeyA"],
        ["ControlRight", "KeyB"],
      ),
    );
    const recorders = wrapper.findAllComponents(HotkeyRecorder);

    recorders[0].vm.$emit("update:modelValue", ["ControlRight", "KeyC"]);
    await flushPromises();

    expect(commands.updateSettings).toHaveBeenCalledOnce();
    const saved = vi.mocked(commands.updateSettings).mock.calls[0][0] as Settings;
    expect(saved.hotkeys.dictation).toEqual(["ControlRight", "KeyC"]);
    expect(saved.hotkeys.translation).toEqual(["ControlRight", "KeyC", "KeyB"]);
    expect(wrapper.text()).not.toContain("neither may contain the other");
  });
});
