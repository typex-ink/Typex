// 模型预设契约：本地 LLM 可手动绑定到问答槽；零配置兜底由 Rust registry 控制。
import { describe, expect, it } from "vitest";
import { presetsForSlot } from "../shared/presets";

describe("模型预设", () => {
  it("问答槽允许手动选择本地 LLM", () => {
    const presets = presetsForSlot("assistant");
    expect(presets.map((p) => p.id)).toContain("local-llm");
  });

  it("STT 槽只列出 STT 预设", () => {
    const presets = presetsForSlot("stt");
    expect(presets.map((p) => p.id)).toContain("local-stt");
    expect(presets.map((p) => p.id)).not.toContain("local-llm");
  });

  it("Xiaomi MiMo 预设锁定原生 ASR adapter 与 base URL", () => {
    const preset = presetsForSlot("stt").find((p) => p.id === "mimo-stt");
    expect(preset).toEqual({
      id: "mimo-stt",
      label: "Xiaomi MiMo",
      kind: "mimo",
      base_url: "https://api.xiaomimimo.com/v1",
      models: ["mimo-v2.5-asr"],
      for: "stt",
    });
  });

  it("新增与升级的云端预设提供最新模型且自定义预设保持在末尾", () => {
    const sttPresets = presetsForSlot("stt");
    expect(sttPresets.at(-1)?.id).toBe("custom-stt");
    expect(sttPresets.find((p) => p.id === "openai-stt")?.models).toEqual([
      "gpt-transcribe",
      "gpt-4o-mini-transcribe",
    ]);
    expect(sttPresets.find((p) => p.id === "mistral-stt")).toEqual({
      id: "mistral-stt",
      label: "Mistral",
      kind: "openai_compat",
      base_url: "https://api.mistral.ai/v1",
      models: ["voxtral-mini-latest"],
      for: "stt",
    });

    const llmPresets = presetsForSlot("polish");
    expect(llmPresets.at(-1)?.id).toBe("custom-llm");
    expect(llmPresets.find((p) => p.id === "openai")?.models).toEqual([
      "gpt-5.4-mini",
      "gpt-5.4",
      "gpt-5.4-nano",
    ]);
    expect(llmPresets.find((p) => p.id === "groq")?.models).toEqual([
      "openai/gpt-oss-20b",
      "openai/gpt-oss-120b",
      "qwen/qwen3.8-27b",
    ]);
    expect(llmPresets.find((p) => p.id === "dashscope")).toEqual({
      id: "dashscope",
      label: "阿里百炼 · DashScope",
      kind: "chat_completions",
      base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      models: ["qwen-flash", "qwen-plus"],
      for: "llm",
    });
    expect(llmPresets.find((p) => p.id === "mimo-llm")).toEqual({
      id: "mimo-llm",
      label: "Xiaomi MiMo",
      kind: "chat_completions",
      base_url: "https://api.xiaomimimo.com/v1",
      models: ["mimo-v2.6-flash", "mimo-v2.6-pro"],
      for: "llm",
    });
  });
});
