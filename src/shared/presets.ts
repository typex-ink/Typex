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
  { id: "openai-stt", label: "OpenAI", kind: "openai_compat", base_url: "https://api.openai.com/v1", models: ["gpt-transcribe", "gpt-4o-mini-transcribe"], for: "stt" },
  { id: "groq-stt", label: "Groq", kind: "openai_compat", base_url: "https://api.groq.com/openai/v1", models: ["whisper-large-v3-turbo", "whisper-large-v3"], for: "stt" },
  { id: "siliconflow-stt", label: "SiliconFlow", kind: "openai_compat", base_url: "https://api.siliconflow.cn/v1", models: ["FunAudioLLM/SenseVoiceSmall", "TeleAI/TeleSpeechASR"], for: "stt" },
  { id: "mimo-stt", label: "Xiaomi MiMo", kind: "mimo", base_url: "https://api.xiaomimimo.com/v1", models: ["mimo-v2.5-asr"], for: "stt" },
  { id: "mistral-stt", label: "Mistral", kind: "openai_compat", base_url: "https://api.mistral.ai/v1", models: ["voxtral-mini-latest"], for: "stt" },
  { id: "volcano-stt", label: "火山引擎 · 豆包（极速版）", kind: "volcengine", base_url: "", models: ["bigmodel"], for: "stt" },
  { id: "custom-stt", label: "自定义", labelKey: "settings.profile.preset_custom", kind: "openai_compat", base_url: "", models: [], for: "stt" },
  // LLM
  { id: "local-llm", label: "本地 · 离线", labelKey: "settings.profile.preset_local", kind: "local", base_url: "", models: [], for: "llm" },
  { id: "openai", label: "OpenAI", kind: "responses", base_url: "https://api.openai.com/v1", models: ["gpt-5.4-mini", "gpt-5.4", "gpt-5.4-nano"], for: "llm" },
  { id: "deepseek", label: "DeepSeek", kind: "chat_completions", base_url: "https://api.deepseek.com/v1", models: ["deepseek-chat", "deepseek-reasoner"], for: "llm" },
  { id: "groq", label: "Groq", kind: "chat_completions", base_url: "https://api.groq.com/openai/v1", models: ["openai/gpt-oss-20b", "openai/gpt-oss-120b", "qwen/qwen3.8-27b"], for: "llm" },
  { id: "dashscope", label: "阿里百炼 · DashScope", kind: "chat_completions", base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1", models: ["qwen-flash", "qwen-plus"], for: "llm" },
  { id: "siliconflow", label: "SiliconFlow", kind: "chat_completions", base_url: "https://api.siliconflow.cn/v1", models: ["deepseek-ai/DeepSeek-V3.2", "Qwen/Qwen3-32B", "Qwen/Qwen3-14B"], for: "llm" },
  { id: "mimo-llm", label: "Xiaomi MiMo", kind: "chat_completions", base_url: "https://api.xiaomimimo.com/v1", models: ["mimo-v2.6-flash", "mimo-v2.6-pro"], for: "llm" },
  { id: "volcano-llm", label: "火山方舟 · 豆包", kind: "chat_completions", base_url: "https://ark.cn-beijing.volces.com/api/v3", models: ["doubao-seed-2-0-lite", "doubao-seed-2-0-pro"], for: "llm" },
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
