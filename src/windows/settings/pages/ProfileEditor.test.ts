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

function sttProfile(): ProviderProfile {
  return {
    id: "stt-openai",
    capability: "stt",
    kind: "openai_compat",
    label: "Groq STT",
    base_url: "https://api.groq.com/openai/v1",
    model: "whisper-large-v3-turbo",
    credentials: { api_key: "sk-existing" },
    extra_headers: {},
    extra_form: {},
    timeout_ms: 30_000,
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

    await wrapper.get('input[placeholder="如 Groq · whisper-turbo"]').setValue("Test STT");
    await wrapper.get('input[placeholder="https://api.example.com/v1"]').setValue("https://api.example.com/v1");
    await wrapper.get('input[placeholder="模型名"]').setValue("whisper-test");
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

  it("新建 STT 档案默认 60 秒且保存编辑后的秒数", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const wrapper = mount(ProfileEditor, {
      attachTo: host,
      props: { capability: "stt", profile: null },
      global: { plugins: [makeI18n("zh-CN")] },
    });

    const timeout = wrapper.get(".timeout-field input");
    expect((timeout.element as HTMLInputElement).value).toBe("60");
    expect(wrapper.text()).toContain("所有使用此档案的语音转文字请求统一生效");

    await wrapper.get('input[placeholder="如 Groq · whisper-turbo"]').setValue("Test STT");
    await wrapper.get('input[placeholder="https://api.example.com/v1"]').setValue("https://api.example.com/v1");
    await wrapper.get('input[placeholder="模型名"]').setValue("whisper-test");
    await wrapper.get("input[type='password']").setValue("sk-test");
    await timeout.setValue("75");

    const save = wrapper.findAll("button").find((button) => button.text().includes("保存"))!;
    await save.trigger("click");
    await flushPromises();

    expect(mockUpsertProfile.mock.calls[0]?.[0].timeout_ms).toBe(75_000);
  });

  it("新建 LLM 档案默认 60 秒且保存编辑后的秒数", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const wrapper = mount(ProfileEditor, {
      attachTo: host,
      props: { capability: "llm", profile: null },
      global: { plugins: [makeI18n("zh-CN")] },
    });

    const timeout = wrapper.get(".timeout-field input");
    expect((timeout.element as HTMLInputElement).value).toBe("60");

    await wrapper.get('input[placeholder="如 Groq · whisper-turbo"]').setValue("Test LLM");
    await wrapper.get('input[placeholder="https://api.example.com/v1"]').setValue("https://api.example.com/v1");
    await wrapper.get('input[placeholder="模型名"]').setValue("test-model");
    await wrapper.get("input[type='password']").setValue("sk-test");
    await timeout.setValue("75");

    const save = wrapper.findAll("button").find((button) => button.text().includes("保存"))!;
    await save.trigger("click");
    await flushPromises();

    expect(mockUpsertProfile.mock.calls[0]?.[0].timeout_ms).toBe(75_000);
  });

  it("现有本地 LLM 档案显示并保留 30 秒调用超时", async () => {
    const profile = llmProfile("local");
    profile.credentials = {};
    const wrapper = mount(ProfileEditor, {
      props: { capability: "llm", profile },
      global: { plugins: [makeI18n("zh-CN")] },
    });

    const timeout = wrapper.get(".timeout-field input");
    expect((timeout.element as HTMLInputElement).value).toBe("30");

    const save = wrapper.findAll("button").find((button) => button.text().includes("保存"))!;
    await save.trigger("click");
    await flushPromises();

    expect(mockUpsertProfile.mock.calls[0]?.[0].timeout_ms).toBe(30_000);
  });

  it("现有毫秒级 STT 超时可显示并保存为小数秒", async () => {
    const profile = sttProfile();
    profile.timeout_ms = 1_500;
    const wrapper = mount(ProfileEditor, {
      props: { capability: "stt", profile },
      global: { plugins: [makeI18n("zh-CN")] },
    });

    const timeout = wrapper.get(".timeout-field input");
    expect((timeout.element as HTMLInputElement).value).toBe("1.5");
    expect(timeout.attributes("step")).toBe("0.001");

    const save = wrapper.findAll("button").find((button) => button.text().includes("保存"))!;
    await save.trigger("click");
    await flushPromises();

    expect(mockUpsertProfile.mock.calls[0]?.[0].timeout_ms).toBe(1_500);
  });

  it("调用超时无法换算为正整数毫秒或超过安全整数时禁用保存", async () => {
    const wrapper = mount(ProfileEditor, {
      props: { capability: "stt", profile: sttProfile() },
      global: { plugins: [makeI18n("zh-CN")] },
    });
    const timeout = wrapper.get(".timeout-field input");
    const save = wrapper.findAll("button").find((button) => button.text().includes("保存"))!;

    for (const invalid of ["", "0", "0.0001", "1.0001", String(Math.floor(Number.MAX_SAFE_INTEGER / 1000) + 1)]) {
      await timeout.setValue(invalid);
      expect(save.attributes("disabled"), `timeout=${invalid}`).toBeDefined();
    }

    await timeout.setValue("1.001");
    expect(save.attributes("disabled")).toBeUndefined();
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
