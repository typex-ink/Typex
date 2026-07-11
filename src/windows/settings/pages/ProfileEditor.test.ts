import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { type ProviderKind, type ProviderProfile } from "@/ipc/bindings";
import ProfileEditor from "./ProfileEditor.vue";

const mockUpsertProfile = vi.hoisted(() =>
  vi.fn(async (_profile: ProviderProfile) => ({ status: "ok" as const })),
);
const mockSetProfileSecret = vi.hoisted(() =>
  vi.fn(async (): Promise<unknown> => ({ status: "ok" as const, data: null })),
);
const mockActivateProfile = vi.hoisted(() =>
  vi.fn(async () => ({ status: "ok" as const, data: null })),
);

vi.mock("@/ipc/bindings", () => ({
  commands: {
    upsertProfile: mockUpsertProfile,
    setProfileSecret: mockSetProfileSecret,
    activateProfile: mockActivateProfile,
    testProfile: vi.fn(async () => ({ status: "ok", data: 10 })),
    deleteProfile: vi.fn(async () => ({ status: "ok" })),
    listLocalModels: vi.fn(async () => ({ status: "ok", data: [] })),
    downloadLocalModel: vi.fn(async () => ({ status: "ok" })),
  },
  events: {
    localDownloadProgressEvent: { listen: vi.fn(async () => vi.fn()) },
  },
}));

function llmProfile(kind: ProviderKind): ProviderProfile {
  return {
    id: `llm-${kind}`,
    capability: "llm",
    kind,
    label: "OpenAI",
    base_url: "https://api.openai.com/v1",
    model: "gpt-5-mini",
    credentials: { api_key: "sk-existing" },
    extra_headers: {},
    extra_form: {},
    timeout_ms: 30000,
    options: {},
  };
}

async function saveWithReasoning(kind: ProviderKind) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const wrapper = mount(ProfileEditor, {
    attachTo: host,
    props: { capability: "llm", profile: llmProfile(kind) },
    global: { plugins: [makeI18n("zh-CN")] },
  });

  const reasoningTrigger = wrapper
    .findAll("button.select")
    .find((button) => button.text().includes("关闭"))!;
  await reasoningTrigger.trigger("click");

  const highOption = [...document.body.querySelectorAll<HTMLButtonElement>(".select-option")]
    .find((option) => option.textContent?.includes("高"))!;
  expect(highOption).toBeTruthy();
  highOption.click();
  await flushPromises();

  const save = wrapper.findAll("button").find((button) => button.text().includes("保存"))!;
  await save.trigger("click");
  await flushPromises();
}

async function saveWithoutChangingReasoning(kind: ProviderKind) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const wrapper = mount(ProfileEditor, {
    attachTo: host,
    props: { capability: "llm", profile: llmProfile(kind) },
    global: { plugins: [makeI18n("zh-CN")] },
  });

  const reasoningTrigger = wrapper
    .findAll("button.select")
    .find((button) => button.text().includes("关闭"))!;
  expect(reasoningTrigger.text()).not.toContain("默认");

  const save = wrapper.findAll("button").find((button) => button.text().includes("保存"))!;
  await save.trigger("click");
  await flushPromises();
}

describe("ProfileEditor", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSetProfileSecret.mockResolvedValue({ status: "ok", data: null });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    document.body.innerHTML = "";
  });

  it("新建档案测试后保存复用同一个 ID", async () => {
    vi.spyOn(Date, "now").mockReturnValueOnce(1_000).mockReturnValueOnce(2_000);
    const host = document.createElement("div");
    document.body.appendChild(host);
    const wrapper = mount(ProfileEditor, {
      attachTo: host,
      props: { capability: "stt", profile: null, assignTo: "stt" },
      global: { plugins: [makeI18n("zh-CN")] },
    });

    const inputs = wrapper.findAll("input");
    await inputs[0].setValue("Test STT");
    await inputs[1].setValue("https://api.example.com/v1");
    await inputs[2].setValue("whisper-test");
    await wrapper.find("input[type='password']").setValue("sk-test");

    const test = wrapper.findAll("button").find((button) => button.text().includes("测试连接"))!;
    await test.trigger("click");
    await flushPromises();
    const save = wrapper.findAll("button").find((button) => button.text().includes("保存"))!;
    await save.trigger("click");
    await flushPromises();

    const savedIds = mockUpsertProfile.mock.calls.map(([profile]) => profile.id);
    expect(savedIds).toHaveLength(2);
    expect(new Set(savedIds).size).toBe(1);
    expect(mockActivateProfile).toHaveBeenNthCalledWith(1, "stt", savedIds[0]);
    expect(mockActivateProfile).toHaveBeenNthCalledWith(2, "stt", savedIds[0]);
  });

  it("Responses 档案可保存思考等级", async () => {
    await saveWithReasoning("responses");

    const saved = mockUpsertProfile.mock.calls[0]?.[0];
    if (!saved) throw new Error("expected upsertProfile call");
    const options = saved.options ?? {};
    expect(options.reasoning_effort).toBe("high");
    expect(options.enable_thinking).toBe(true);
  });

  it("未配置思考等级时默认保存为关闭", async () => {
    await saveWithoutChangingReasoning("responses");

    const saved = mockUpsertProfile.mock.calls[0]?.[0];
    if (!saved) throw new Error("expected upsertProfile call");
    const options = saved.options ?? {};
    expect(options.reasoning_effort).toBe("none");
    expect(options.enable_thinking).toBe(false);
  });

  it("OpenAI 兼容档案可保存思考等级", async () => {
    await saveWithReasoning("chat_completions");

    const saved = mockUpsertProfile.mock.calls[0]?.[0];
    if (!saved) throw new Error("expected upsertProfile call");
    const options = saved.options ?? {};
    expect(options.reasoning_effort).toBe("high");
    expect(options.enable_thinking).toBe(true);
  });

  it("密钥写入失败时显示上游错误信息", async () => {
    mockSetProfileSecret.mockResolvedValueOnce({
      status: "error",
      error: {
        code: "internal",
        message: "写入设置失败",
      },
    });
    const host = document.createElement("div");
    document.body.appendChild(host);
    const wrapper = mount(ProfileEditor, {
      attachTo: host,
      props: { capability: "llm", profile: llmProfile("responses") },
      global: { plugins: [makeI18n("zh-CN")] },
    });

    await wrapper.find("input[type='password']").setValue("sk-new");
    const save = wrapper.findAll("button").find((button) => button.text().includes("保存"))!;
    await save.trigger("click");
    await flushPromises();

    expect(mockSetProfileSecret).toHaveBeenCalledWith("llm-responses", "api_key", "sk-new");
    expect(wrapper.text()).toContain("写入设置失败");
    expect(wrapper.emitted("saved")).toBeUndefined();
  });

  it("旧 keyring 引用不算已保存密钥", async () => {
    const profile = llmProfile("responses");
    profile.credentials = { api_key: "keyring://typex/llm/openai/api_key" };
    const host = document.createElement("div");
    document.body.appendChild(host);
    const wrapper = mount(ProfileEditor, {
      attachTo: host,
      props: { capability: "llm", profile },
      global: { plugins: [makeI18n("zh-CN")] },
    });

    const save = wrapper.findAll("button").find((button) => button.text().includes("保存"))!;

    expect(save.attributes("disabled")).toBeDefined();
  });
});
