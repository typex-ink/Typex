import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import type { Settings } from "@/ipc/bindings";
import { useSettingsStore } from "@/stores/settings";
import GeneralPage from "./GeneralPage.vue";

const updateSettings = vi.hoisted(() => vi.fn());

vi.mock("@/ipc/bindings", () => ({
  commands: { updateSettings },
  events: {
    settingsChangedEvent: { listen: vi.fn(async () => () => {}) },
  },
}));

function makeSettings(chimesEnabled = true, chimesVolume = 0.6): Settings {
  return {
    general: {
      language: "system",
      theme: "system",
      autostart: true,
      chimes_enabled: chimesEnabled,
      chimes_volume: chimesVolume,
      proxy_mode: "system",
      proxy_url: "",
      model_download_source: "auto",
      check_updates: true,
      update_channel: "stable",
    },
  } as Settings;
}

function mountPage(chimesEnabled = true, chimesVolume = 0.6) {
  const pinia = createPinia();
  setActivePinia(pinia);
  const store = useSettingsStore();
  store.settings = makeSettings(chimesEnabled, chimesVolume);
  return mount(GeneralPage, {
    global: { plugins: [pinia, makeI18n("en")] },
  });
}

describe("GeneralPage sound cue volume", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    updateSettings.mockImplementation(async (settings: Settings) => ({
      status: "ok",
      data: settings,
    }));
  });

  it("shows percent and persists the slider value as a 0-1 gain", async () => {
    const wrapper = mountPage();
    const slider = wrapper.get<HTMLInputElement>('input[type="range"]');

    expect(slider.element.value).toBe("60");
    expect(wrapper.text()).toContain("60%");

    await slider.setValue("85");
    await flushPromises();

    const saved = updateSettings.mock.calls.at(-1)?.[0] as Settings;
    expect(saved.general.chimes_volume).toBeCloseTo(0.85);
    expect(wrapper.text()).toContain("85%");
  });

  it("disables the slider while muted and preserves its value when re-enabled", async () => {
    const wrapper = mountPage(false, 0.35);
    const slider = wrapper.get<HTMLInputElement>('input[type="range"]');
    const toggle = wrapper.findAll('button[role="switch"]')[1];

    expect(slider.element.disabled).toBe(true);
    expect(slider.element.value).toBe("35");
    expect(toggle.attributes("aria-checked")).toBe("false");

    await toggle.trigger("click");
    await flushPromises();

    expect(toggle.attributes("aria-checked")).toBe("true");
    const saved = updateSettings.mock.calls.at(-1)?.[0] as Settings;
    expect(saved.general.chimes_enabled).toBe(true);
    expect(saved.general.chimes_volume).toBeCloseTo(0.35);
    expect(slider.element.disabled).toBe(false);
  });
});
