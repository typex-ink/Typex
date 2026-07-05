# AGENTS.md

给所有编程智能体（Claude Code、Codex、Copilot 等）的项目工作指南。

## 项目是什么

Typex：开源跨平台 AI 语音输入软件（Tauri 2 + Rust 后端，Vue 3 + TS + Tailwind v4 前端）。按住全局快捷键说话 → STT 转写 → 轻量 LLM 整理/翻译 → 注入光标处。BYOK、无服务端、零遥测。

## 事实来源与文档纪律

- **设计书 [`docs/`](docs/)（01–09 章）是唯一事实来源**：行为语义以 02（功能规格）/05（UX 规格）为准，Provider 协议以 03 为准，模块归属与分层规则以 07 为准，测试规范以 08 为准，UI 外观以 `docs/mockups/ui-mono.html` 为准。
- **代码与设计书冲突时，先改设计书再改代码**；改动 IPC 契约、配置 schema、状态机行为的提交必须同步更新对应章节。
- 实现进度记录在 `ROADMAP.md`：完成一个 checkpoint 就标 `[x]`（附日期）并单独提交一次。

## 常用命令

```bash
pnpm tauri dev                                     # 开发运行（含 vite）
pnpm build                                         # vue-tsc 类型检查 + vite build
pnpm test                                          # vitest（src/**/*.test.ts）
cargo test --manifest-path src-tauri/Cargo.toml    # Rust 全部测试（单测 + wiremock 集成）
cargo test --manifest-path src-tauri/Cargo.toml session   # 按名过滤运行单个/一组测试
pnpm gen:ipc                                       # 重新生成 src/ipc/bindings.ts
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml
```

合入前全部命令必须绿：fmt / clippy -D warnings / cargo test / pnpm build / pnpm test（与 CI 一致）。

## 结构组成（07 章的落地形态）

```
src-tauri/src/
├── app/            # Tauri 壳：commands/events/tray/windows —— 只做胶水，禁止业务逻辑
├── orchestrator/   # 唯一的业务流程所有者：session.rs 纯函数状态机 + mod.rs 执行器
├── hotkey/ audio/ inject/ selection/ providers/ settings/ history/   # 领域服务，互不依赖
├── platform/       # OS 探测、权限；不依赖任何 service；unsafe 只允许出现在这里
├── types/          # 跨层共享结构体（serde + specta derive），禁止裸 JSON
└── error.rs        # TypexError + ErrorCode（= 前端 i18n key）
src/
├── windows/        # 每窗口一个 Vite 入口：hud/assistant/settings/onboarding/home
├── components/     # 组件库（Button/Toggle/ProviderCard/HotkeyRecorder…）
├── stores/ composables/ i18n/ shared/
├── ipc/bindings.ts # tauri-specta 生成物（gitignore；禁止手改）
└── styles/tokens.css  # 设计 token 唯一落地处
```

**分层依赖单向且禁止横向**：前端 → app → orchestrator → service → platform。service 之间互相不 import（audio 不知道 inject 的存在）；只有 orchestrator 能同时调用多个 service。任何「这个没法测」优先怀疑代码放错了层。

### 关键机制

- **状态机**（`orchestrator/session.rs`）：`advance(state, event, threshold) -> (state, Vec<Effect>)`，零 IO 纯函数。全部业务规则（长短按判定、组合键让路、重按忽略、失败恢复、session_id 竞态丢弃）都在这里。执行器（`orchestrator/mod.rs`）dispatch Effect 到 service，异步回调带 session_id 回状态机校验。
- **Provider 层**：`SttProvider`/`LlmProvider` 两个 trait；`registry.rs` 按 settings profile 惰性构建缓存、变更精准失效。错误统一分类 `ProviderError`（Auth 不重试；RateLimited/Server/Network/Timeout 走 `http.rs::with_retry` 指数退避 ×2——**重试逻辑只存在这一处**，别在 orchestrator 重复实现）。
- **状态所有权**：业务状态（会话、配置）真身在 Rust；前端 Pinia 只是订阅缓存（`settings://changed` 回流），前端不自行推导会话状态。HUD 完全由 `session://snapshot` 事件驱动渲染。
- **密钥**：settings.json 只存 `keyring://typex/<slot>/<profile-id>/<field>` 引用，明文只进 OS 凭据库（`settings/secrets.rs`；测试用 `MemoryStore`）。密钥不随 profile JSON 往返前端，单独走 `set_profile_secret`。
- **日志**：不记录转写文本内容与任何凭据——只记长度与耗时。这是隐私承诺的一部分，review 时对新日志语句保持警惕。
- **提示词**：内置模板在 `providers/llm/prompt.rs`（与设计书 03 §3.4 逐字一致）；渲染规则=「行内含可选占位符且值缺失时整行省略」。前端设置页的默认模板文本与 Rust 侧重复定义，**改模板必须同步两处**。

## IPC 契约纪律

- 所有 command/event 在 Rust 侧定义（`app/commands.rs` / `app/events.rs`），经 tauri-specta 生成 TS。
- **改了任何 `#[tauri::command]`、Event 或 IPC 载荷类型后必须跑 `pnpm gen:ipc`**，否则前端编译用旧 bindings。
- 前端禁止手写 `invoke("...")` 字符串——唯一例外是 `src/windows/hud/ipc.ts`（HUD 极简纪律，见下）。
- 新 command 记得同时加进 `runner.rs` 的 `collect_commands![]`，否则运行时 404。
- 新增 `ErrorCode` 变体时必须同步 `src/i18n/zh-CN.json` 与 `en.json` 的 `error.*` 文案——有契约测试卡这一点（`src/__tests__/i18n-errorcodes.test.ts`）。

## 测试规范（08 章，合入前置条件）

- **测试与代码同 PR**：新增/修改行为必须携带对应测试（纯文案/注释/样式除外）。先写（或同时写）测试与类型签名，再写实现。
- **测行为，不测实现**：断言公开接口的输入输出与副作用，不断言内部调了什么函数。
- **确定性**：不碰真实网络/时间/键盘/麦克风——HTTP 用 wiremock，时间作参数注入，keyring 用 MemoryStore，音频用合成 PCM。flaky 测试按 bug 处理，不允许 retry 掩盖。
- **分级要求**：`orchestrator/session.rs`、`providers/*`、hotkey 判定逻辑属关键模块——每条行为规则要有具名测试（08 §3.1 场景清单是需求的镜像，**改行为先更新场景表，表里没有的行为不存在**）。`platform/`、`app/`（纯转发）豁免单测，靠人工回归。
- **禁止弱化测试转绿**：修改断言/删用例/加 ignore 必须在提交说明中单独解释理由。
- 提示词（整理/翻译/判定）的行为变更需对照 `docs/fixtures/denoise-cases.md` 语料评测，不进 PR CI。

## UI 铁律（ADR-18 纯墨方案）

- 全部颜色引用 `src/styles/tokens.css` 变量，**组件内禁止硬编码色值**。
- 界面只有黑白灰；唯一彩色是 `--recording`/`--error`（录音红/错误红），不得引入其他色相与渐变。
- 选中态 = `--sel-bg` 灰底 + 600 字重，**禁止反色实底**；每个页面只有主按钮一处实底反色；链接与 Ghost 按钮用下划线表达。
- 实现/修改任何界面前先对照 `docs/mockups/ui-mono.html` 对应片段；深浅双主题都要可用；组件需键盘可达、焦点环用 `--focus-ring`。
- **HUD 窗口极简纪律**：不引 Pinia/路由/Markdown/vue-i18n 运行时（i18n 用静态 JSON 直取）；隐藏时零活动（动画只在录音时运行）。CI 有 chunk gzip < 150KB 断言。
- 动效尊重 `prefers-reduced-motion`；文案全部走 i18n 资源（中英双语）。

## 平台注意（macOS 为当前主战场，代码保持三平台 trait 可扩展）

- HUD 必须是 nonactivating NSPanel（`tauri-nspanel` git 依赖）——普通窗口抢焦点会毁掉注入目标。
- rdev 监听需要辅助功能权限，未授权时**静默无事件**不报错；右 ⌥ 在 rdev 里叫 `AltGr`（无 AltRight 变体）。
- cpal Stream 非 Send：录音流活在专属线程，callback 内只做拷贝转发（禁止分配/锁/日志）。
- 剪贴板注入前保存原内容、注入后恢复（仅文本保真）；粘贴前需短延迟（慢应用）。
- 禁止在持锁状态下 await 网络调用。

## 开发技巧

- 开发期跳过设置界面直配 STT：`TYPEX_STT_API_KEY=xxx`（可选 `TYPEX_STT_BASE_URL`/`TYPEX_STT_MODEL`），启动时写入 `env-stt` 档案。
- 调试直开窗口：`TYPEX_OPEN=home|settings|assistant ./src-tauri/target/debug/typex`（仅 debug 构建）。
- macOS 实机验证 UI：按窗口 ID 截图 `screencapture -x -l <window-id>`，窗口 ID 用 Swift `CGWindowListCopyWindowInfo` 查。**勿用全局鼠标点击自动化**——会干扰正在使用电脑的用户。

## 提交约定

- Conventional Commits（`feat(audio): …`，scope 用 07 章模块名：hotkey/audio/inject/selection/providers/settings/history/orchestrator/app/platform）；提交信息中文。
- 产品名一律写作 **Typex**（仅首字母大写）；bundle id `ink.typex.app`。
