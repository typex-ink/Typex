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
  translation: string[] = [...new Set([...dictation, ...assistant])],
): Settings {
  return {
    schema_version: 9,
    hotkeys: {
      dictation,
      assistant,
      translation,
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

function keyboard(
  type: "keydown" | "keyup",
  code: string,
  location = 0,
  modifiers: KeyboardEventInit = {},
) {
  window.dispatchEvent(new KeyboardEvent(type, { ...modifiers, code, location, bubbles: true }));
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
    expect(wrapper.text()).toContain("cannot contain each other");

    recorders[1].vm.$emit("update:modelValue", ["ControlRight", "KeyA"]);
    await nextTick();
    expect(commands.updateSettings).not.toHaveBeenCalled();
  });

  it("saves one chord without recalculating independent translation", async () => {
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
    expect(saved.hotkeys.translation).toEqual(["ControlRight", "KeyA", "KeyB"]);
    expect(wrapper.text()).not.toContain("cannot contain each other");
  });

  it("renders three recorders and persists an independent translation chord", async () => {
    const wrapper = mountPage();
    const recorders = wrapper.findAllComponents(HotkeyRecorder);
    expect(recorders).toHaveLength(3);

    recorders[2].vm.$emit("update:modelValue", ["F13", "ContextMenu", "Menu"]);
    await flushPromises();

    const saved = vi.mocked(commands.updateSettings).mock.calls[0][0] as Settings;
    expect(saved.hotkeys.dictation).toEqual(["ControlRight"]);
    expect(saved.hotkeys.assistant).toEqual(["AltRight"]);
    expect(saved.hotkeys.translation).toEqual(["F13", "Menu"]);
  });

  it("blocks a translation chord identical to dictation", async () => {
    const wrapper = mountPage();
    const recorders = wrapper.findAllComponents(HotkeyRecorder);

    recorders[2].vm.$emit("update:modelValue", ["ControlRight"]);
    await nextTick();

    expect(commands.updateSettings).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("translation cannot exactly match");
  });

  it("allows translation to be a strict subset of dictation", async () => {
    const wrapper = mountPage(
      makeSettings(
        ["ControlRight", "KeyA"],
        ["AltRight"],
        ["ControlRight", "KeyA", "AltRight"],
      ),
    );
    const recorders = wrapper.findAllComponents(HotkeyRecorder);

    recorders[2].vm.$emit("update:modelValue", ["ControlRight"]);
    await flushPromises();

    expect(commands.updateSettings).toHaveBeenCalledOnce();
    const saved = vi.mocked(commands.updateSettings).mock.calls[0][0] as Settings;
    expect(saved.hotkeys.translation).toEqual(["ControlRight"]);
  });

  it("allows translation to contain keys from both other chords", async () => {
    const wrapper = mountPage();
    const recorders = wrapper.findAllComponents(HotkeyRecorder);

    recorders[2].vm.$emit("update:modelValue", ["ControlRight", "AltRight", "F13"]);
    await flushPromises();

    expect(commands.updateSettings).toHaveBeenCalledOnce();
    const saved = vi.mocked(commands.updateSettings).mock.calls[0][0] as Settings;
    expect(saved.hotkeys.translation).toEqual(["ControlRight", "AltRight", "F13"]);
  });

  it("records right Ctrl plus right Shift when WebView drops the Shift keydown", async () => {
    const wrapper = mountPage(
      makeSettings(["ControlRight"], ["ShiftRight"], ["KeyW"]),
    );
    const recorders = wrapper.findAllComponents(HotkeyRecorder);

    await recorders[2].get("button").trigger("click");
    keyboard("keydown", "ControlRight", 2);
    keyboard("keydown", "Unidentified", 2);
    keyboard("keyup", "ControlRight", 2, { shiftKey: true });
    await nextTick();
    expect(commands.updateSettings).not.toHaveBeenCalled();

    keyboard("keyup", "ShiftLeft", 2);
    await flushPromises();

    expect(commands.updateSettings).toHaveBeenCalledOnce();
    const saved = vi.mocked(commands.updateSettings).mock.calls[0][0] as Settings;
    expect(saved.hotkeys.translation).toEqual(["ControlRight", "ShiftRight"]);
    expect(wrapper.text()).not.toContain("translation cannot exactly match");
  });
});
