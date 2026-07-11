import { mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";
import { makeI18n } from "@/i18n";
import HotkeyRecorder from "./HotkeyRecorder.vue";

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: () => "windows",
}));

function keyboard(type: "keydown" | "keyup", code: string, location = 0) {
  window.dispatchEvent(new KeyboardEvent(type, { code, location, bubbles: true }));
}

describe("HotkeyRecorder", () => {
  it("records a complete physical chord using canonical KeyIds", async () => {
    const wrapper = mount(HotkeyRecorder, {
      props: { modelValue: ["F13"] },
      global: { plugins: [makeI18n("en")] },
    });

    await wrapper.get("button").trigger("click");
    expect(wrapper.get("button").text()).toBe("Press keys…");
    expect(wrapper.find(".callout").exists()).toBe(false);
    keyboard("keydown", "ControlRight");
    keyboard("keydown", "Digit1");
    keyboard("keyup", "Digit1");
    await nextTick();

    expect(wrapper.emitted("update:modelValue")?.at(-1)?.[0]).toEqual([
      "ControlRight",
      "Digit1",
    ]);
    expect(wrapper.get("button").text()).toBe("Change");
    expect(wrapper.text()).not.toContain("Recording…");
  });

  it("maps browser aliases and preserves press order", async () => {
    const wrapper = mount(HotkeyRecorder, {
      props: { modelValue: ["F13"] },
      global: { plugins: [makeI18n("en")] },
    });

    await wrapper.get("button").trigger("click");
    keyboard("keydown", "AltRight");
    keyboard("keydown", "ContextMenu");
    keyboard("keyup", "ContextMenu");

    expect(wrapper.emitted("update:modelValue")?.at(-1)?.[0]).toEqual([
      "AltRight",
      "Menu",
    ]);
  });

  it("records right Shift when WebView reports a left code with right location", async () => {
    const wrapper = mount(HotkeyRecorder, {
      props: { modelValue: ["F13"] },
      global: { plugins: [makeI18n("en")] },
    });

    await wrapper.get("button").trigger("click");
    keyboard("keydown", "ShiftLeft", 2);
    keyboard("keyup", "ShiftLeft", 2);
    await nextTick();

    expect(wrapper.emitted("update:modelValue")?.at(-1)?.[0]).toEqual(["ShiftRight"]);
  });

  it("renders historical aliases with current platform labels", () => {
    const wrapper = mount(HotkeyRecorder, {
      props: { modelValue: ["AltGr", "Return", "LeftArrow"] },
      global: { plugins: [makeI18n("en")] },
    });

    expect(wrapper.text()).toContain("Right Alt");
    expect(wrapper.text()).toContain("Enter");
    expect(wrapper.text()).toContain("←");
  });

  it("cancels capture on Escape without changing the binding", async () => {
    const wrapper = mount(HotkeyRecorder, {
      props: { modelValue: ["F13"] },
      global: { plugins: [makeI18n("en")] },
    });

    await wrapper.get("button").trigger("click");
    expect(wrapper.get("button").text()).toBe("Press keys…");
    expect(wrapper.find(".callout").exists()).toBe(false);
    keyboard("keydown", "Escape");
    await nextTick();

    expect(wrapper.emitted("update:modelValue")).toBeUndefined();
    expect(wrapper.get("button").text()).toBe("Change");
    expect(wrapper.text()).not.toContain("Recording…");
  });
});
