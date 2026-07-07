import { mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { commands } from "@/ipc/bindings";
import DebugPage from "./DebugPage.vue";

vi.mock("@/ipc/bindings", () => ({
  commands: {
    openOnboardingWindow: vi.fn(async () => ({ status: "ok", data: null })),
  },
}));

describe("DebugPage", () => {
  it("点击按钮重新打开引导页", async () => {
    const wrapper = mount(DebugPage, {
      global: { plugins: [makeI18n("zh-CN")] },
    });

    await wrapper.find("button").trigger("click");

    expect(commands.openOnboardingWindow).toHaveBeenCalledOnce();
    expect(wrapper.text()).toContain("已打开引导页");
  });
});
