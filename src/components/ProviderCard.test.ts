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
    const details = JSON.stringify({
      error: {
        message: "Upstream request failed",
        details: { error: { message: "model not found" } },
        request_id: "req-123",
      },
    });
    mockTestProfile.mockResolvedValue({
      status: "error",
      error: {
        code: "auth_error",
        message: "Upstream request failed",
        details,
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
    expect(wrapper.get(".test-tip-details").text()).toBe(
      JSON.stringify(JSON.parse(details), null, 2),
    );
    expect(wrapper.get(".test-tip-details").attributes("tabindex")).toBe("0");
  });

  it("下一次测试成功后清除旧错误详情", async () => {
    mockTestProfile
      .mockResolvedValueOnce({
        status: "error",
        error: { code: "invalid_request", message: "failed", details: "failed" },
      })
      .mockResolvedValueOnce({ status: "ok", data: 42 });
    const wrapper = mount(ProviderCard, {
      props: { profile: profile() },
      global: { plugins: [makeI18n("zh-CN")] },
    });
    const testButton = wrapper.findAll("button").find((button) => button.text() === "测试")!;

    await testButton.trigger("click");
    await flushPromises();
    expect(wrapper.find(".test-tip-details").exists()).toBe(true);

    await wrapper.findAll("button").find((button) => button.text() === "测试失败")!.trigger("click");
    await flushPromises();
    expect(wrapper.find(".test-tip").exists()).toBe(false);
    expect(wrapper.get(".lat").text()).toContain("42ms");
  });
});
