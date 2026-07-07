// 预设模板（03 §6：前端内置数据、仅作表单填充器、无推荐标注——ADR-21）
import type { ProviderCapability, ProviderKind, SlotKind } from "@/ipc/bindings";

export interface Preset {
  id: string;
  label: string;
  /** 通用词（自定义/自建）走 i18n；厂商名保持原样 */
  labelKey?: string;
  kind: ProviderKind;
  base_url: string;
  /** 建议模型（可改） */
  models: string[];
  /** 适用槽位类别 */
  for: "stt" | "llm";
}

export const PRESETS: Preset[] = [
  // STT
  { id: "local-stt", label: "本地 · 离线", labelKey: "settings.profile.preset_local", kind: "local", base_url: "", models: [], for: "stt" },
  { id: "openai-stt", label: "OpenAI", kind: "openai_compat", base_url: "https://api.openai.com/v1", models: ["gpt-4o-mini-transcribe", "whisper-1"], for: "stt" },
  { id: "groq-stt", label: "Groq", kind: "openai_compat", base_url: "https://api.groq.com/openai/v1", models: ["whisper-large-v3-turbo"], for: "stt" },
  { id: "siliconflow-stt", label: "SiliconFlow", kind: "openai_compat", base_url: "https://api.siliconflow.cn/v1", models: ["FunAudioLLM/SenseVoiceSmall"], for: "stt" },
  { id: "volcano-stt", label: "火山引擎 · 豆包（极速版）", kind: "volcengine", base_url: "", models: ["bigmodel"], for: "stt" },
  { id: "custom-stt", label: "自定义", labelKey: "settings.profile.preset_custom", kind: "openai_compat", base_url: "", models: [], for: "stt" },
  // LLM
  { id: "local-llm", label: "本地 · 离线", labelKey: "settings.profile.preset_local", kind: "local", base_url: "", models: [], for: "llm" },
  { id: "openai", label: "OpenAI", kind: "responses", base_url: "https://api.openai.com/v1", models: ["gpt-5-mini", "gpt-5"], for: "llm" },
  { id: "deepseek", label: "DeepSeek", kind: "chat_completions", base_url: "https://api.deepseek.com/v1", models: ["deepseek-chat"], for: "llm" },
  { id: "groq", label: "Groq", kind: "chat_completions", base_url: "https://api.groq.com/openai/v1", models: ["llama-3.3-70b-versatile"], for: "llm" },
  { id: "siliconflow", label: "SiliconFlow", kind: "chat_completions", base_url: "https://api.siliconflow.cn/v1", models: ["Qwen/Qwen3-14B"], for: "llm" },
  { id: "volcano-llm", label: "火山方舟 · 豆包", kind: "chat_completions", base_url: "https://ark.cn-beijing.volces.com/api/v3", models: [], for: "llm" },
  { id: "openrouter", label: "OpenRouter", kind: "chat_completions", base_url: "https://openrouter.ai/api/v1", models: [], for: "llm" },
  { id: "ollama", label: "Ollama / 自建", labelKey: "settings.profile.preset_self_hosted", kind: "chat_completions", base_url: "http://localhost:11434/v1", models: [], for: "llm" },
  { id: "custom-llm", label: "自定义", labelKey: "settings.profile.preset_custom", kind: "chat_completions", base_url: "", models: [], for: "llm" },
];

export function presetsForCapability(capability: ProviderCapability): Preset[] {
  return PRESETS.filter((p) => p.for === capability);
}

export function presetsForSlot(slot: SlotKind): Preset[] {
  return presetsForCapability(slot === "stt" ? "stt" : "llm");
}
