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
});
