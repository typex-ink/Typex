# 03 · 模型接入层（Provider 规范）

> Typex 产品设计书 · 第三章
> 本章是 F-4（BYOK 配置）的实现规范，基于 2026-07 对各厂商 API 的实测调研。所有 Provider 代码位于 `src-tauri/src/providers/`。

---

## 1. 总体设计

两类 Provider，各自一个 Rust trait；云端实现**只依赖 HTTP(S)，不携带厂商 SDK**，本地实现（`local`，v1.1）为进程内推理：

```
SttProvider ── 音频 → 文本                    （F-1/2/3 共用）
LlmProvider ── 文本 → 文本（单次调用，流式）   （F-9 整理、F-2 翻译、F-3 处理/问答 三个槽位共用）
```

> v1 没有 AgentProvider：F-3 已定为单轮问答/单次文本处理（[ADR-1](09-decisions.md)），一次 LLM 调用即可，无工具循环。四个模型槽位（STT / 整理 / 翻译 / 问答）在实现上只对应这两个 trait——后三个槽只是 LlmProvider 的不同配置 + 不同内置提示词。

设计原则：

1. **以事实标准为默认协议**：STT 默认 OpenAI 转录格式，LLM/Agent 默认 OpenAI Chat Completions 与 Responses——一个实现覆盖最长尾的厂商与自建服务。
2. **专用 adapter 按需增补**：不兼容的厂商（火山/豆包）写薄适配器，共享同一 trait。
3. **能力声明**：每个 Provider 暴露 `capabilities()`（是否流式、最大音频时长、是否支持 prompt/热词…），上层据此调整行为，而不是 if-else 厂商名。
4. **统一错误分类**：`AuthError / RateLimited / Timeout / InvalidRequest / ServerError / NetworkError`——HUD 文案与重试策略按分类处理（RateLimited/ServerError/Network 自动重试，Auth 不重试直接引导检查密钥）。

## 2. SttProvider

```rust
pub trait SttProvider: Send + Sync {
    async fn transcribe(&self, audio: AudioInput, opts: SttOptions) -> Result<Transcript, ProviderError>;
    fn capabilities(&self) -> SttCapabilities;
    // P2: fn stream(&self) -> Option<Box<dyn SttStreamSession>>;
}

pub struct AudioInput { pub wav_16k_mono: Vec<u8>, pub duration_ms: u64 }
pub struct SttOptions { pub language: Option<String>, pub prompt: Option<String>, pub temperature: Option<f32> }
pub struct Transcript { pub text: String, pub detected_language: Option<String> }
```

内部统一喂 **16 kHz 单声道 WAV**（所有调研厂商都接受 WAV；OpenAI/Groq 服务端本来就会降采样到 16 kHz，本地重采样不损失精度且体积最小）。响应只依赖 `text` 字段（最小交集）。

### 2.1 内置实现一：`openai_compat`（默认）

覆盖 OpenAI、Groq、SiliconFlow、Azure OpenAI（URL 模板差异）、Mistral（Voxtral）以及各类自建服务（vLLM、FunASR server、speaches 等）。

```
POST {base_url}/audio/transcriptions
Authorization: Bearer {api_key}
Content-Type: multipart/form-data

form fields:
  file            = audio.wav (必填)
  model           = {model}   (必填, 如 gpt-4o-mini-transcribe / whisper-large-v3-turbo)
  language        = zh        (可选, ISO-639-1, 提升精度与速度)
  prompt          = ...       (可选, 术语引导 → F-10 个人词典入口)
  response_format = json      (固定 json, 最大兼容)
  temperature     = 0         (可选)
```

响应：`{ "text": "..." }`。注意事项：

- 单文件 25 MB 上限（OpenAI/Groq）→ orchestrator 在 VAD 静音处切片（[07 §7.4](07-code-architecture.md)）。
- SiliconFlow 等只实现了子集（忽略 language/prompt）——不报错，静默降级，符合「最小交集」原则。
- 配置项允许附加自定义 form 字段与 HTTP header（应对网关与厂商扩展）。

### 2.2 内置实现二：`volcengine`（豆包 / 火山引擎）

火山是**完全自有协议**，不兼容 OpenAI。v1 走「大模型录音文件识别——极速版」：同步 HTTP、JSON body、base64 音频，最适合语音输入场景（免去二进制 WebSocket 协议）。

```
POST https://openspeech.bytedance.com/api/v3/auc/bigmodel/recognize/flash
X-Api-App-Key:     {app_id}          # 控制台 APP ID
X-Api-Access-Key:  {access_token}    # Access Token
X-Api-Resource-Id: volc.bigasr.auc_turbo
X-Api-Request-Id:  {uuid}
Content-Type: application/json

{
  "user":    { "uid": "typex" },
  "audio":   { "format": "wav", "data": "<base64 wav>" },
  "request": { "model_name": "bigmodel", "enable_punc": true, "enable_itn": true }
}
```

- 成功判定：响应 header `X-Api-Status-Code: 20000000`；文本在响应体 `result.text`。
- **凭据是双字段（AppKey + AccessToken）**——Provider 配置 schema 必须支持多凭据字段（见 §6）。
- 热词经 `corpus` 参数（F-10 预留）。
- 流式识别（二进制 WS 帧协议，`wss://openspeech.bytedance.com/api/v3/sauc/bigmodel`）留待 P2 实时字幕需求出现时再实现。

### 2.3 内置实现三：`local`（本地推理，v1.1，[ADR-20](09-decisions.md)/[ADR-22](09-decisions.md)）

不走 HTTP，进程内推理，实现同一个 `SttProvider` trait。按硬件档位提供两条引擎路线：

- **Qwen3-ASR（标准/性能档）**：llama.cpp（qwen3vl 音频架构，官方 ggml-org GGUF）跑 `Qwen3-ASR-0.6B`（Q8_0 主模型 + mmproj 约 1.0 GB）/ `Qwen3-ASR-1.7B`（Q8_0 主模型 + mmproj 约 2.5 GB，仅 GPU 加速可用时提供——纯 CPU 低于实时）。52 语言 + 22 中文方言，1.7B 为开源 ASR SOTA。**注意**：llama.cpp 音频支持仍标 experimental、长音频有已知 bug——所有音频先过 VAD 切片成短分段再转写（本来就是 F-1 的路径），规避该问题。
  - Qwen3-ASR 的 llama.cpp / OpenAI-compatible 网关输出可能带 `language Chinese<asr_text>...` 包装；Provider 层必须剥离 `language ...<asr_text>` 前缀、把语言填入 `Transcript.detected_language`，不得把包装文本传给 orchestrator / 前端 / 注入层。
- **SenseVoice（轻量档）**：sherpa-onnx（官方 Rust crate，静态链接）+ `SenseVoice-Small int8`（约 230 MB）。非自回归，CPU 实时数倍速——弱机器上唯一保证实时的选项；自带 VAD 可复用。热词经 sherpa hotwords 接口（F-10 预留）。
- whisper.cpp 降为可选扩展（Qwen3-ASR 的语言覆盖已够长尾）。
- `capabilities()` 报告：不限音频时长（本地无 25 MB 上限）；错误分类只剩 `InvalidRequest`/模型未下载。
- 模型文件由**模型下载管理器**负责（见 §8）：不随安装包分发，按需下载。

### 2.4 P2 扩展位

- `deepgram` / `elevenlabs`：各约百行的薄 adapter（改鉴权头、上传方式）。
- 流式：各家协议互不兼容（OpenAI Realtime 事件 JSON / 火山二进制帧 / Deepgram 裸推）；唯一准标准是 OpenAI Realtime（阿里 Qwen3-ASR 已模仿）。故流式做成可选 capability，v1 用「快 Provider + 一次性转写」已可达到近实时体验。

## 3. LlmProvider（整理 / 翻译 / 问答）

```rust
pub trait LlmProvider: Send + Sync {
    /// 单轮任务型调用；流式返回 delta
    fn complete(&self, req: LlmRequest) -> impl Stream<Item = Result<LlmDelta, ProviderError>>;
    fn capabilities(&self) -> LlmCapabilities;
}
pub struct LlmRequest { pub system: String, pub messages: Vec<Msg>, pub temperature: f32, pub max_tokens: Option<u32> }
```

### 3.1 内置实现一：`chat_completions`（OpenAI 兼容格式）

```
POST {base_url}/chat/completions
Authorization: Bearer {api_key}
{ "model": "...", "messages": [{"role":"system",...},{"role":"user",...}],
  "stream": true, "temperature": 0.3 }
```

SSE `data:` 行解析 `choices[0].delta.content`。覆盖 OpenAI、DeepSeek、Groq、SiliconFlow、OpenRouter、Ollama、火山方舟（豆包 LLM 也提供 OpenAI 兼容端点）等几乎全部生态。

Qwen3 / 千问类模型可能默认输出 `<think>...</think>` 推理块，语音助手会表现为长时间“思考”且把内部推理显示到回答弹窗。Provider 层必须做两件事：

- `profiles[].options.enable_thinking` 是用户可配置布尔值，默认 `false`；对已知支持该扩展的 Qwen 兼容 Chat Completions 端点（SiliconFlow、DashScope/阿里兼容模式等），以及用户显式保存该选项的自定义 Qwen 端点，请求体发送顶层 `enable_thinking`。
- 不论端点是否支持该参数，所有 LLM 流式 delta 在进入 orchestrator 前都要过滤完整或跨 chunk 分片的 `<think>...</think>` 块；内部推理不得出现在助手弹窗、整理/翻译结果或注入文本中。

### 3.2 内置实现二：`responses`（OpenAI Responses 格式）

```
POST {base_url}/responses
Authorization: Bearer {api_key}
{ "model": "...", "input": [ {"role":"system","content":[...]}, {"role":"user","content":[...]} ],
  "stream": true }
```

SSE 事件流：处理 `response.output_text.delta`（增量文本）、`response.completed`、`response.failed`。这是 OpenAI 的新主协议（也是 Codex 生态的唯一 wire 协议），必须一等支持。

### 3.3 内置实现三：`local`（本地推理，v1.1，[ADR-20](09-decisions.md)）

进程内推理，实现同一个 `LlmProvider` trait（流式返回 delta 与云端一致）：

- **引擎**：llama.cpp（`llama-cpp-2` 绑定——GGUF 生态最全，Apple Silicon Metal 加速成熟；与本地 STT 的 Qwen3-ASR 共用同一引擎，[ADR-22](09-decisions.md)）。
- **模型**：Qwen3.5 小模型系列 instruct GGUF（0.8B / 2B / 4B，Q4_K_M），按硬件档位下载（见 §8）。Apache 2.0，多语言，中文分词效率高。
- **槽位策略**：本地 LLM 可绑定到「文本整理」「翻译模型」「问答模型」槽；零配置路径只自动指向整理/翻译，问答槽默认仍为空并显示配置引导。性能档设备可在设置中手动把问答槽指向本地 4B 级模型（[ADR-22](09-decisions.md)）。
- **运行时策略**：模型常驻内存或「录音开始时预热」（设置可选）；冷加载约 1–3 s。上下文窗口按需 4 K 即可（整理/翻译都是短输入）。
- **思考模式**：`profiles[].options.enable_thinking` 默认 `false`；本地 Qwen LLM 在最后一条用户消息末尾注入 `/think` 或 `/no_think` 控制词。即便模型仍输出 `<think>...</think>`，Provider 层也会在流式 delta 进入 orchestrator 前过滤。
- `capabilities()`：流式 = 是；错误分类只剩 `InvalidRequest`/模型未下载/内存不足。

### 3.4 内置提示词与占位符（可在高级设置中覆盖）

提示词是**含占位符的模板**（`PromptKit` 负责渲染），用户自定义时使用同一套占位符——这是自定义能力的正式接口：

| 槽位 | 占位符 | 含义 | 必需 |
|---|---|---|---|
| 文本整理 | `{transcript}` | STT 原始转写文本 | ✅ |
| | `{dictionary}` | 个人词典词表（F-10，未启用时该段整体省略） | — |
| 翻译 | `{transcript}` | STT 原始转写文本 | ✅ |
| | `{source_language}` / `{target_language}` | 源语言 / 目标语言（来自翻译设置） | ✅ |
| | `{bidirectional_source}` / `{bidirectional_target}` | 双向翻译子句用的语言对（「双向翻译」关闭时值不注入 → 该行整体省略） | — |
| 问答（F-3a/b） | `{instruction}` | 用户的语音指令 / 问题转写 | ✅ |
| | `{selection}` | 选中文本（无选区时该段整体省略） | — |

规则：编辑器中占位符高亮显示；保存时校验**必需占位符必须出现**（缺失则禁用保存 + 行内报错）；含可选占位符的行在运行时按「值不存在则整行省略」处理；「恢复默认」一键回到内置模板。

**文本整理（F-9，「文本整理」槽）**：

```
你是语音转写的后处理引擎。输入是一段语音识别原始文本，输出整理后的文本。
规则：删除语气词与无意义重复；修复标点与断句；
识别说话人的自我修正（如「不对/应该是/我是说」），只保留最终意图；
将口述的格式指令（另起一段、列成清单）转为真实格式；
不增删信息、不改变语言、不替换用词——整理不是改写；
只输出结果本身。
以下专有名词按原样保留：{dictionary}
【原始转写】{transcript}
```

**翻译（F-2，「翻译模型」槽）**：

```
你是一个专业翻译引擎。输入是语音转写文本，先在心中还原说话者的真实意图
（忽略语气词、重复与中途改口），再将其从{source_language}翻译为{target_language}。
规则：只输出译文本身；不解释、不加引号、不加任何前后缀；
保留原文的段落、列表与换行结构；语气与正式程度与原文一致；
若原文已经是{bidirectional_target}，则翻译为{bidirectional_source}（双向翻译）。
【原文】{transcript}
```

（双向子句独立使用 `{bidirectional_*}` 占位符：设置中关闭「双向翻译」时不注入这两个值，按可选段规则该行整体省略——开关由此生效。）

**文本处理（F-3a，「问答模型」槽）**：

```
你是文本处理引擎。用户选中了一段文本并口述了处理要求。
若要求是对文本的加工（改写/翻译/精简/格式化等）：只输出加工后的文本本身，
不解释、不寒暄，结果将直接替换原文；
若要求实际上是就这段文本提问：以「ANSWER:」开头输出简洁回答。
【选中文本】{selection}
【处理要求】{instruction}
```

（`ANSWER:` 前缀是 F-3a「改写 vs 回答」的判定信号：有前缀 → 回答弹窗展示、不替换选区；无前缀 → 直接替换选区、不弹窗。流首部即可判定，见 [02 F-3a](02-features.md)。）

**语音问答（F-3b，「问答模型」槽）**：

```
你是 Typex 语音助手。用户通过语音提出一个问题，这是单轮问答。
回答应直接、简洁、可立即使用；默认使用用户提问的语言。
用户当前选中的内容作为上下文：{selection}
【问题】{instruction}
```

## 4. F-3 的实现说明（无 Agent 层）

F-3 不引入新的 Provider 类型：

- **F-3a 文本处理** = `LlmProvider.complete(系统提示词 + 选中文本 + 语音指令)` 一次调用；「替换选区」是 Rust 注入服务在**收到完整结果后**执行的本地动作，不是模型工具。
- **F-3b 问答** = 同一 trait 的另一组提示词，流式渲染到回答弹窗。
- 单轮语义：请求内不携带历史消息。

### 4.1 已评估并否决的方案（背景记录，详见 [ADR-1](09-decisions.md)）

曾评估以 OpenAI Codex SDK 作为内嵌 Agent 引擎，调研结论（2026-07）：Codex SDK 本质是 codex CLI（约 15–25 MB/平台 Rust 二进制）的包装，需子进程常驻；2026-02 起仅支持 Responses wire 协议（接第三方 OpenAI 兼容端点需网关中转）；provider 配置只认用户级 `~/.codex/config.toml`；其核心价值（OS 沙箱下的文件/命令操作、ChatGPT 订阅额度）与 Typex 的文本场景错配。**最终决策：v1 不接入任何 Agent SDK，F-3 为单轮 LLM 调用**；重度 Agent 需求引导用户使用 Codex / Claude Code 等专业工具。若未来需要，`providers/` 的目录结构不阻碍新增 AgentProvider。

## 5. 配置槽位与 Provider 的关系

```
设置槽位            实现走向
──────────────────────────────────────────
语音转文字   ──▶  SttProvider（openai_compat | volcengine | local）
文本整理     ──▶  LlmProvider + 整理提示词（推荐轻量快模型；可用 local）
翻译模型     ──▶  LlmProvider + 翻译提示词（可用 local）
问答模型     ──▶  LlmProvider + 处理/问答提示词（推荐强模型；可手动选择 local）
```

共用规则：LLM 三槽默认共用同一个「连接配置」（base_url + 密钥），仅模型名与提示词不同；用户可随时把某个槽切换为完全独立的配置。onboarding 只需配置 STT + 一个 LLM 连接即全功能可用。

**多配置档案（[ADR-21](09-decisions.md)）**：`profiles[]` 天然支持同一槽位多个档案，`slots.*.active_profile` 只是指针——切换 = 改指针，不删档案。切换入口：设置页 ProviderCard「切换 ▾」与托盘「模型 ▸」子菜单（听写/翻译/问答 三组），即时生效。

**零配置兜底（[ADR-20](09-decisions.md)）**：STT / 整理 / 翻译三槽在用户未配置任何档案时默认指向 `local` 档案（模型已下载的前提下）；问答槽无兜底，未配置时助手功能显示配置引导。本地与云端可槽位级混搭（如 STT 本地 + 翻译云端）。

## 6. 配置 Schema（settings.json 中的形态）

```jsonc
{
  "schema_version": 1,
  "general": {
    "model_download_source": "auto" // auto | huggingface | modelscope；仅影响本地模型下载
  },
  "slots": {
    "stt":       { "active_profile": "groq-fast" },
    "polish":    { "active_profile": "deepseek" },   // 文本整理
    "translate": { "active_profile": "deepseek" },   // 与整理共用连接，仅提示词不同
    "assistant": { "active_profile": "openai-gpt" }  // 问答
  },
  "profiles": [
    {
      "id": "groq-fast", "slot": "stt", "kind": "openai_compat",
      "label": "Groq · whisper-large-v3-turbo",
      "base_url": "https://api.groq.com/openai/v1",
      "model": "whisper-large-v3-turbo",
      "credentials": { "api_key": "keyring://typex/stt/groq-fast" },
      "extra_headers": {}, "extra_form": {}, "timeout_ms": 30000,
      "options": { "language": "auto", "temperature": 0 }
    },
    {
      "id": "doubao", "slot": "stt", "kind": "volcengine",
      "label": "豆包 · 极速版",
      "base_url": "https://openspeech.bytedance.com",
      "model": "bigmodel",
      "credentials": {
        "app_key":    "keyring://typex/stt/doubao/app_key",
        "access_key": "keyring://typex/stt/doubao/access_key"
      },
      "options": { "resource_id": "volc.bigasr.auc_turbo", "enable_punc": true, "enable_itn": true }
    },
    {
      "id": "deepseek", "slot": "polish|translate", "kind": "chat_completions",
      "label": "DeepSeek V3",
      "base_url": "https://api.deepseek.com/v1", "model": "deepseek-chat",
      "credentials": { "api_key": "keyring://typex/llm/deepseek" },
      "options": { "temperature": 0.2 }
    },
    {
      "id": "openai-gpt", "slot": "assistant", "kind": "responses",
      "label": "OpenAI · gpt-5",
      "base_url": "https://api.openai.com/v1", "model": "gpt-5",
      "credentials": { "api_key": "keyring://typex/assistant/openai-gpt" }
    },
    {
      "id": "local-qwen-asr", "slot": "stt", "kind": "local",
      "label": "本地 · Qwen3-ASR-0.6B",
      "model": "qwen3-asr-0.6b-q8",           // 指向模型库中的条目，无 base_url、无凭据
      "options": { "language": "auto" }
    },
    {
      "id": "local-qwen", "slot": "polish|translate", "kind": "local",
      "label": "本地 · Qwen3.5-2B",
      "model": "qwen3.5-2b-q4",
      "options": { "preload": "on_record" }    // resident | on_record
    }
  ]
}
```

要点：

- `kind` 决定 adapter；`credentials` 是 **map 结构**（为火山双凭据这类情况设计），值一律是 keyring 引用，明文不落盘。
- LLM `options.enable_thinking` 控制 thinking 模式，缺省为 `false`；云端只对支持端点发送 `enable_thinking` 参数，避免破坏 OpenAI / Responses 等严格协议；本地 Qwen LLM 通过 `/think` / `/no_think` 控制词实现。
- **预设模板**（前端内置数据，非后端逻辑）：OpenAI / Groq / SiliconFlow / 火山·豆包 / DeepSeek / OpenRouter / Ollama —— 选中即预填 `kind/base_url/model` 与凭据字段表单，用户只贴密钥。
- 「测试连接」：STT 槽发内置 2 秒样音（assets 内置，中文「你好，Typex」），LLM 槽发 `ping` 单词请求；展示延迟与分类后的错误。

## 7. 各厂商兼容性速查（配置预设的依据）

| 厂商 | STT | LLM | 备注 |
|---|---|---|---|
| OpenAI | openai_compat | chat_completions / responses | 基准 |
| Groq | openai_compat | chat_completions | STT 极快（whisper-turbo），语音输入首选预设 |
| SiliconFlow | openai_compat（子集） | chat_completions | SenseVoice 中文；参数支持面窄 |
| 火山引擎 · 豆包 | **volcengine adapter** | chat_completions（方舟端点） | STT 双凭据；LLM 是 OpenAI 兼容的 |
| DeepSeek | — | chat_completions | 整理/翻译高性价比 |
| OpenRouter | — | chat_completions / responses | 聚合网关 |
| Ollama / 自建 | openai_compat（如 speaches） | chat_completions | 本地/内网 |
| **本地 · 离线（v1.1）** | **local**（Qwen3-ASR GGUF / SenseVoice·sherpa-onnx 按档位） | **local**（llama.cpp + Qwen3.5 0.8B/2B/4B，仅整理/翻译槽） | 零配置兜底，[ADR-20](09-decisions.md)/[ADR-22](09-decisions.md)；硬件分档 + 按需下载（§8） |
| 阿里 DashScope | P2（chat-completions 变体传音频） | chat_completions 兼容端点 | STT 格式特殊，v1 不做 |
| Deepgram / ElevenLabs | P2 薄 adapter | — | 非 OpenAI 格式但协议简单 |

## 8. 模型下载管理器（v1.1，配合 `local`）

本地模型不随安装包分发（安装包只含推理引擎，约 +30–60 MB）；由下载管理器按需获取：

- **模型库清单**（内置 JSON，随应用更新）：每个模型条目 = id、显示名、用途（stt/llm）、文件列表、字节数、SHA-256、许可证、双源 URL、**最低硬件要求（RAM/是否需 GPU 加速）**。
  - v1.1 起始清单（[ADR-22](09-decisions.md)）：STT = `sense-voice-small-int8`（约 230 MB）/ `qwen3-asr-0.6b-q8`（Q8_0，约 1.0 GB）/ `qwen3-asr-1.7b-q8`（Q8_0，约 2.5 GB，要求 GPU 加速）；LLM = `qwen3.5-0.8b-q4`（约 0.5 GB）/ `qwen3.5-2b-q4`（约 1.3 GB）/ `qwen3.5-4b-q4`（约 2.7 GB）。
- **硬件分档推荐**：首次下载时探测设备（RAM 总量、CPU 核数、Metal/CUDA/Vulkan 可用性），自动勾选推荐档——轻量（SenseVoice + 0.8B，约 0.8 GB）/ 标准（ASR-0.6B + 2B，约 2.3 GB）/ 性能（ASR-1.7B + 4B，约 5.3 GB）；用户可改档或单选模型。探测逻辑在 Rust 侧（`sysinfo` + 各加速后端探测），结果同时展示在诊断页。
- **双源**：HuggingFace 与 ModelScope 镜像；默认自动（HuggingFace 优先、失败换源），可在设置-模型服务-模型管理页底部固定为 HuggingFace 或 ModelScope。
- **下载行为**：断点续传（HTTP Range）、SHA-256 校验、失败换源重试；进度经 Tauri event 推送 UI（onboarding 第 3 步与设置-模型服务页共用同一进度组件）。
- **存储**：`{app_data_dir}/models/{model_id}/`；设置-模型服务页显示占用体积、支持删除；删除被槽位引用的模型时警告。
- **边界**：下载是本地 Provider 唯一的网络行为；不做后台自动更新模型（用户手动触发），与「零上报」承诺一致。
