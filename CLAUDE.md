# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目是什么

Typex：开源跨平台 AI 语音输入软件（Tauri 2 + Rust 后端，Vue 3 + TS + Tailwind v4 前端）。按住全局快捷键说话 → STT 转写 → 轻量 LLM 整理/翻译 → 注入光标处。BYOK、无服务端、零遥测。

**设计书是唯一事实来源**，位于 `/Users/daodaoneko/Documents/Typex/docs/`（01–09 章 + `mockups/ui-mono.html`）。行为语义以 02/05 章为准，模块归属以 07 章为准，UI 外观以 mockup 为准。**代码与设计书冲突时，先改设计书再改代码。** 实现进度记录在 `ROADMAP.md`：每完成一个 checkpoint 就标记 `[x]` 并提交一次。

## 常用命令

```bash
pnpm tauri dev                                     # 开发运行（含 vite）
pnpm build                                         # vue-tsc + vite build（前端类型检查在这里）
pnpm test                                          # vitest（src/**/*.test.ts）
cargo test --manifest-path src-tauri/Cargo.toml    # Rust 全部测试（单测 + wiremock 集成）
cargo test --manifest-path src-tauri/Cargo.toml session   # 按名过滤单个测试
pnpm gen:ipc                                       # 重新生成 src/ipc/bindings.ts（= cargo test export_bindings）
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml
```

- **改了任何 `#[tauri::command]` / Event / IPC 载荷类型后必须跑 `pnpm gen:ipc`**，否则前端编译用的是旧 bindings。bindings.ts 在 gitignore 中，前端禁止手写 `invoke("...")` 字符串（HUD 的 `src/windows/hud/ipc.ts` 是唯一例外——HUD 极简纪律不引全量 bindings）。
- 开发期跳过设置界面直配 STT：`TYPEX_STT_API_KEY=xxx`（可选 `TYPEX_STT_BASE_URL`/`TYPEX_STT_MODEL`），启动时会写入 `env-stt` 档案。
- 调试打开窗口：`TYPEX_OPEN=home|settings|assistant ./src-tauri/target/debug/typex`（仅 debug 构建）。
- macOS 实机验证 UI：窗口按 ID 截图 `screencapture -x -l <window-id>`，窗口 ID 用 Swift `CGWindowListCopyWindowInfo` 查（勿用全局鼠标点击自动化——会干扰用户）。

## 架构（07 章的落地形态）

分层单向依赖：`前端 → app（胶水）→ orchestrator → service（互不依赖）→ platform`。

- **`orchestrator/session.rs` 是全项目核心**：纯函数状态机 `advance(state, event, threshold) -> (state, Vec<Effect>)`，零 IO。所有业务规则（长短按 350ms、组合键让路、重按忽略、失败恢复、session_id 竞态丢弃）都在这里，且每条规则有具名测试（08 §3.1 场景清单）。**改行为先加/改测试场景，表里没有的行为不存在。**
- **`orchestrator/mod.rs` 是执行器**：dispatch Effect 到各 service，持有会话音频/转写稿存储（失败重试不丢话）。异步回调全部带 session_id 回状态机校验。
- **service 之间互相不 import**（audio 不知道 inject 的存在）；只有 orchestrator 能同时调多个 service。
- **Provider 层**（`providers/`）：`SttProvider`/`LlmProvider` 两个 trait；`registry.rs` 按 settings 中的 profile 惰性构建缓存实例，settings 变更精准失效。错误统一分类为 `ProviderError`（Auth 不重试；RateLimited/Server/Network/Timeout 经 `http.rs::with_retry` 指数退避 ×2——重试逻辑只在这一处）。
- **状态所有权**：业务状态（会话、配置）真身在 Rust；前端 Pinia 只是订阅缓存（`settings://changed` 事件回流）。HUD 完全由 `session://snapshot` 驱动渲染，前端无自己的状态机。
- **密钥**：settings.json 只存 `keyring://typex/<slot>/<profile-id>/<field>` 引用，明文只进 OS 凭据库（`settings/secrets.rs`，测试用 `MemoryStore`）。日志不记转写内容与任何凭据（只记长度/耗时）。
- **提示词**：内置模板在 `providers/llm/prompt.rs`（03 §3.4 逐字），占位符渲染规则=「可选占位符所在行，值缺失时整行省略」；F-3a 改写 vs 回答判定靠 `ANSWER:` 前缀。前端设置页的默认模板文本与 Rust 侧重复定义，改模板要同步两处。

## UI 铁律（ADR-18 纯墨方案）

- 全部颜色引用 `src/styles/tokens.css` 变量，**组件内禁止硬编码色值**。
- 界面只有黑白灰；唯一彩色是 `--recording`/`--error`（录音红/错误红），不得引入其他色相与渐变。
- 选中态 = `--sel-bg` 灰底 + 600 字重，**禁止反色实底**；每个页面只有主按钮一处实底反色。
- 实现任何界面前先对照 `docs/mockups/ui-mono.html` 对应片段；双主题都要可用。
- HUD 窗口纪律：不引 Pinia/路由/Markdown/vue-i18n 运行时（chunk gzip < 150KB，CI 有断言）。

## 测试纪律（08 章）

- 测试与代码同 PR；测行为不测实现；不碰真实网络/时间/键盘（时间注入参数、HTTP 用 wiremock、keyring 用 MemoryStore）。
- 关键模块（session.rs、providers/*、hotkey 判定器）要求场景级具名测试全覆盖。
- 前端契约测试：`src/__tests__/i18n-errorcodes.test.ts` 从 bindings.ts 提取 ErrorCode 全集断言 zh-CN/en 文案齐全——Rust 加错误码必须同时加两份文案。
- 集成测试位于 `src-tauri/tests/providers_wiremock.rs`（请求构造 + SSE 解析 + 错误分类 + 重试行为两个方向都断言）。

## 平台注意（macOS 当前主战场）

- HUD 必须是 nonactivating NSPanel（`tauri-nspanel` git 依赖），普通窗口抢焦点会毁掉注入。
- rdev 监听要辅助功能权限，未授权时**静默无事件**不报错；右 ⌥ 在 rdev 里叫 `AltGr`（无 AltRight 变体）。
- cpal Stream 非 Send：录音流活在专属线程，callback 只拷贝转发。
- 剪贴板注入前保存原内容注入后恢复（仅文本保真）；Windows/Linux 后端留有 trait 位但未实测。

## 提交约定

Conventional Commits（`feat(audio): …`，scope 用 07 章模块名）；提交信息中文。改 IPC 契约/配置 schema/状态机行为的提交必须同步更新 ROADMAP 或设计书对应章节。当前仓库无 remote，只做本地提交。
