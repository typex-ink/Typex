import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { type ProviderProfile } from "@/ipc/bindings";
import ProviderCard from "./ProviderCard.vue";

const mockTestProfile = vi.hoisted(() => vi.fn());

vi.mock("@/ipc/bindings", () => ({
  commands: {
    testProfile: mockTestProfile,
  },
}));

function profile(): ProviderProfile {
  return {
    id: "p-1",
    capability: "llm",
    kind: "chat_completions",
    label: "tf",
    base_url: "https://tokenflux.dev/v1",
    model: "gpt-5.4-mini",
    credentials: {},
    extra_headers: {},
    extra_form: {},
    timeout_ms: 30000,
    options: {},
  };
}

describe("ProviderCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("测试失败时用按钮态承载错误，完整报错放入悬停浮层", async () => {
    mockTestProfile.mockResolvedValue({
      status: "error",
      error: {
        code: "auth_error",
        message: "Upstream request failed",
      },
    });
    const wrapper = mount(ProviderCard, {
      props: { profile: profile() },
      global: { plugins: [makeI18n("zh-CN")] },
    });

    const testButton = wrapper.findAll("button").find((button) => button.text() === "测试")!;
    await testButton.trigger("click");
    await flushPromises();

    expect(mockTestProfile).toHaveBeenCalledWith("p-1");
    expect(wrapper.findAll("button").some((button) => button.text() === "测试失败")).toBe(true);
    expect(wrapper.find(".lat").exists()).toBe(false);
    expect(wrapper.find(".test-tip").text()).toContain("鉴权/访问被拒（401/403）");
    expect(wrapper.find(".test-tip").text()).toContain("Upstream request failed");
  });
});
