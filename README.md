# Typex

> **说，即所得。** 开源、跨平台的 AI 语音输入软件。
> Speak. It types. — Open-source AI voice input for Windows / macOS / Linux.

按住一个键说话，松开后**被整理干净的文字**出现在任何应用的光标处。Typex 还能翻译你说的话（说中文、出英文）、听懂你对选中文本的口述指令、随时语音提问得到回答。

**BYOK（Bring Your Own Key）· 无账号 · 无中间服务器**：你的声音只发送到你自己配置的模型端点，Typex 没有服务器，代码可审计。

## 功能

1. **语音输入** — 按住 `右 ⌘`（macOS）/ `右 Ctrl`（Win/Linux）说话，STT 转写 + 轻量模型整理（去语气词、修标点、保留改口后的最终意图），注入光标处。
2. **语音翻译** — 按住 `右 ⌘ + 右 ⌥` 说 A 语言、上屏 B 语言；双向自动判向。
3. **文本处理与语音问答** — 选中文本 + 按住 `右 ⌥` 口述指令 = 原地改写；或随时呼出助手面板单轮问答。

## 开发

```bash
pnpm install
pnpm tauri dev          # 开发运行
pnpm tauri build        # 构建安装包

# 测试
cargo test --manifest-path src-tauri/Cargo.toml   # Rust（单测 + wiremock 集成）
pnpm test                                          # 前端（vitest）
pnpm gen:ipc                                       # 重新生成 IPC bindings
```

开发期临时配置 STT（跳过设置界面）：

```bash
TYPEX_STT_BASE_URL=https://api.groq.com/openai/v1 \
TYPEX_STT_API_KEY=gsk_xxx \
TYPEX_STT_MODEL=whisper-large-v3-turbo \
pnpm tauri dev
```

## 架构

Tauri 2 · Rust（rdev / cpal / enigo / arboard …）· Vue 3 + TypeScript + Tailwind CSS v4。

设计书（唯一事实来源）位于独立文档仓库；实现进度见 [`ROADMAP.md`](ROADMAP.md)。

## 离线能力（v1.1 预览）

无需任何 API 密钥，语音转写与整理/翻译可完全在本机推理（SenseVoice / Qwen3-ASR / Qwen3.5，Apache 2.0）：

```bash
cargo build --manifest-path src-tauri/Cargo.toml --features local-models
```

- 功能锁在 Cargo feature `local-models` 后，默认构建零依赖、零影响。
- 模型不随安装包分发，应用内按需下载（HuggingFace / ModelScope 双源、断点续传、SHA-256 校验）；按设备硬件推荐轻量 / 标准 / 性能三档。
- 模型下载是本地 Provider 唯一的网络行为——零上报承诺不变。

## 隐私

- 音频只发往**你自己配置的端点**；除此之外（更新检查除外，可关）零网络请求、零遥测。
- 密钥存 OS 凭据库（Keychain / Credential Manager / Secret Service），绝不明文落盘。
- 历史记录仅存本机 SQLite，不含音频，可一键关闭并清空。
- 日志不记录转写内容与任何凭据。

## 链接

官网（建设中）：[typex.ink](https://typex.ink)

## 许可证

GPL-3.0（见 [LICENSE](LICENSE)）。
