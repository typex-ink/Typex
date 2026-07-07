import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { type ProviderKind, type ProviderProfile } from "@/ipc/bindings";
import ProfileEditor from "./ProfileEditor.vue";

const mockUpsertProfile = vi.hoisted(() =>
  vi.fn(async (_profile: ProviderProfile) => ({ status: "ok" as const })),
);

vi.mock("@/ipc/bindings", () => ({
  commands: {
    upsertProfile: mockUpsertProfile,
    setProfileSecret: vi.fn(async () => ({ status: "ok" })),
    activateProfile: vi.fn(async () => ({ status: "ok" })),
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
    credentials: { api_key: "keyring://typex/llm/openai/api_key" },
    extra_headers: {},
    extra_form: {},
    timeout_ms: 30000,
    options: {},
  };
}

async function saveWithReasoning(kind: ProviderKind) {
  const wrapper = mount(ProfileEditor, {
    props: { capability: "llm", profile: llmProfile(kind) },
    global: { plugins: [makeI18n("zh-CN")] },
  });
  const reasoningSelect = wrapper.findAll("select").at(-1)!;
  expect(reasoningSelect.text()).toContain("高");

  await reasoningSelect.setValue("high");
  const save = wrapper.findAll("button").find((button) => button.text().includes("保存"))!;
  await save.trigger("click");
  await flushPromises();
}

describe("ProfileEditor", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("Responses 档案可保存思考等级", async () => {
    await saveWithReasoning("responses");

    const saved = mockUpsertProfile.mock.calls[0]?.[0];
    if (!saved) throw new Error("expected upsertProfile call");
    const options = saved.options ?? {};
    expect(options.reasoning_effort).toBe("high");
    expect(options.enable_thinking).toBe(true);
  });

  it("OpenAI 兼容档案可保存思考等级", async () => {
    await saveWithReasoning("chat_completions");

    const saved = mockUpsertProfile.mock.calls[0]?.[0];
    if (!saved) throw new Error("expected upsertProfile call");
    const options = saved.options ?? {};
    expect(options.reasoning_effort).toBe("high");
    expect(options.enable_thinking).toBe(true);
  });
});
