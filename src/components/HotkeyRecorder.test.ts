import { mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";
import { makeI18n } from "@/i18n";
import HotkeyRecorder from "./HotkeyRecorder.vue";

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: () => "windows",
}));

function keyboard(
  type: "keydown" | "keyup",
  code: string,
  location = 0,
  modifiers: KeyboardEventInit = {},
) {
  window.dispatchEvent(new KeyboardEvent(type, { ...modifiers, code, location, bubbles: true }));
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

  it("pairs right Shift when WebView reports different sides on keydown and keyup", async () => {
    const wrapper = mount(HotkeyRecorder, {
      props: { modelValue: ["F13"] },
      global: { plugins: [makeI18n("en")] },
    });

    await wrapper.get("button").trigger("click");
    keyboard("keydown", "ShiftLeft", 2);
    keyboard("keyup", "ShiftLeft", 1);
    await nextTick();

    expect(wrapper.emitted("update:modelValue")?.at(-1)?.[0]).toEqual(["ShiftRight"]);
    expect(wrapper.get("button").text()).toBe("Change");
  });

  it("recovers a chord key from keyup when WebView drops its keydown", async () => {
    const wrapper = mount(HotkeyRecorder, {
      props: { modelValue: ["KeyW"] },
      global: { plugins: [makeI18n("en")] },
    });

    await wrapper.get("button").trigger("click");
    keyboard("keydown", "ControlRight", 2);
    keyboard("keydown", "Unidentified", 2);
    keyboard("keyup", "ControlRight", 2, { shiftKey: true });
    await nextTick();
    expect(wrapper.emitted("update:modelValue")).toBeUndefined();

    keyboard("keyup", "ShiftLeft", 2);
    await nextTick();

    expect(wrapper.emitted("update:modelValue")?.at(-1)?.[0]).toEqual([
      "ControlRight",
      "ShiftRight",
    ]);
    expect(wrapper.get("button").text()).toBe("Change");
  });

  it("ignores an unmatched keyup when no unidentified keydown was observed", async () => {
    const wrapper = mount(HotkeyRecorder, {
      props: { modelValue: ["F13"] },
      global: { plugins: [makeI18n("en")] },
    });

    await wrapper.get("button").trigger("click");
    keyboard("keyup", "KeyA");
    await nextTick();

    expect(wrapper.emitted("update:modelValue")).toBeUndefined();
    expect(wrapper.get("button").text()).toBe("Press keys…");
    keyboard("keydown", "Escape");
    await nextTick();
    expect(wrapper.get("button").text()).toBe("Change");
  });

  it("cancels recovery when the final modifier keyup never arrives", async () => {
    vi.useFakeTimers();
    const wrapper = mount(HotkeyRecorder, {
      props: { modelValue: ["F13"] },
      global: { plugins: [makeI18n("en")] },
    });

    try {
      await wrapper.get("button").trigger("click");
      keyboard("keydown", "ControlRight", 2);
      keyboard("keydown", "Unidentified", 2);
      keyboard("keyup", "ControlRight", 2, { shiftKey: true });
      await nextTick();
      expect(wrapper.get("button").text()).toBe("Press keys…");

      vi.advanceTimersByTime(2_000);
      await nextTick();

      expect(wrapper.emitted("update:modelValue")).toBeUndefined();
      expect(wrapper.get("button").text()).toBe("Change");
    } finally {
      wrapper.unmount();
      vi.useRealTimers();
    }
  });

  it("cancels capture on window blur without changing the binding", async () => {
    const wrapper = mount(HotkeyRecorder, {
      props: { modelValue: ["F13"] },
      global: { plugins: [makeI18n("en")] },
    });

    await wrapper.get("button").trigger("click");
    keyboard("keydown", "ControlRight", 2);
    window.dispatchEvent(new Event("blur"));
    await nextTick();

    expect(wrapper.emitted("update:modelValue")).toBeUndefined();
    expect(wrapper.get("button").text()).toBe("Change");
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
