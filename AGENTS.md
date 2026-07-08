# AGENTS.md

给所有编程智能体（Claude Code、Codex、Copilot 等）的项目工作指南。

## 文档与规范

- 长期维护文档集 [docs/](docs/README.md) 是唯一事实来源；先读文档集 README，再按任务需要阅读具体章节。
- 代码与文档冲突时，先更新对应文档，再改代码。
- 实现进度不写入文档集，使用 GitHub Issues / Projects / Milestones 管理。

## 技术栈

- 后端 / 桌面壳：Tauri 2 + Rust。
- 前端：Vue 3 + TypeScript + Tailwind CSS v4。
- IPC：tauri-specta 生成 TypeScript bindings。

## 项目布局

```text
src-tauri/src/
├── app/            # Tauri 壳：commands/events/tray/windows，只做胶水
├── orchestrator/   # 业务流程所有者：session.rs 状态机 + mod.rs 执行器
├── hotkey/ audio/ inject/ selection/ providers/ settings/ history/
├── platform/       # OS 探测、权限与平台专用胶水
├── types/          # 跨层共享结构体（serde + specta derive）
└── error.rs        # TypexError + ErrorCode

src/
├── windows/        # 每窗口一个 Vite 入口：hud/assistant/settings/onboarding/home
├── components/     # 组件库
├── stores/ composables/ i18n/ shared/
├── ipc/bindings.ts # tauri-specta 生成物（gitignore，禁止手改）
└── styles/tokens.css

docs/               # 长期维护文档集，结构见 docs/README.md
```

## 通用规则

- 分层依赖单向：前端 → app → orchestrator → service → platform。service 之间不横向依赖；只有 orchestrator 能同时调用多个 service。
- `app/` 只做 Tauri 胶水；业务流程归 `orchestrator/`。
- `src/ipc/bindings.ts` 是生成物，禁止手改。
- 不记录转写文本内容、音频内容、密钥或凭据；日志只记长度、耗时、错误分类等非敏感信息。
- 改 IPC 契约、配置 schema、状态机行为、Provider wire shape、UI token 或错误码时，必须同步更新对应章节。
- 不要删除、回滚或覆盖用户已有改动，除非用户明确要求。

## 常用命令

```bash
pnpm install
pnpm tauri dev                                     # 开发运行
pnpm build                                         # vue-tsc 类型检查 + vite build
pnpm test                                          # vitest
pnpm gen:ipc                                       # 重新生成 src/ipc/bindings.ts

cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml session
```

合入前与 CI 对齐的最低检查：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm build
pnpm test
```

## 常见工作流

### 改功能行为

1. 先更新对应规格：功能行为看 [02](docs/02-features.md)，UX 看 [05](docs/05-ux-spec.md)，架构边界看 [06](docs/06-code-architecture.md)。
2. 同步测试：状态机、Provider、hotkey 等关键模块按 [07](docs/07-testing.md) 增补具名用例。
3. 再改实现。
4. 跑相关测试；行为面较广时跑完整 Rust/前端测试。

### 改 IPC

1. 在 Rust 侧改 command/event/type。
2. 新 command 加进 `runner.rs` 的 `collect_commands![]`。
3. 跑 `pnpm gen:ipc`。
4. 前端只通过生成 bindings 调用；不要手写 `invoke("...")` 字符串，HUD 的极简 IPC 例外见 `src/windows/hud/ipc.ts`。

### 改错误码

1. Rust 侧新增 `ErrorCode`。
2. 同步 `src/i18n/zh-CN.json` 与 `src/i18n/en.json` 的 `error.*` 文案。
3. 跑 `pnpm test -- i18n-errorcodes` 或完整 `pnpm test`。

### 改 Provider

1. 协议、配置 schema、提示词先更新 [03](docs/03-model-providers.md)。
2. 重试逻辑只放在 `providers/http.rs::with_retry`。
3. HTTP 行为用 wiremock 或契约快照覆盖，不打真实网络。
4. 改提示词时同步前端默认模板，并用 `docs/fixtures/` 语料做本地评测。

### 改 UI

1. 先对照 [04 设计系统](docs/04-design-system.md) 和 [05 UX 规格](docs/05-ux-spec.md)。
2. 颜色只引用 `src/styles/tokens.css` 变量。
3. 深浅主题、键盘可达、focus ring、`prefers-reduced-motion` 都要确认。
4. HUD 保持轻量：不引 Pinia、路由、Markdown、vue-i18n 运行时。

## 开发技巧

跳过设置界面直配 STT：

```bash
TYPEX_STT_API_KEY=xxx pnpm tauri dev
```

可选：

```bash
TYPEX_STT_BASE_URL=https://api.groq.com/openai/v1
TYPEX_STT_MODEL=whisper-large-v3-turbo
```

调试时直接打开指定窗口：

```bash
TYPEX_OPEN=home ./src-tauri/target/debug/typex
TYPEX_OPEN=settings ./src-tauri/target/debug/typex
TYPEX_OPEN=assistant ./src-tauri/target/debug/typex
```

本地模型默认启用；如需裁剪推理引擎依赖：

```bash
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features
```

## 平台验证

- macOS 是当前主战场，代码保持三平台 trait 可扩展。
- HUD 必须是 nonactivating NSPanel。
- rdev 未获辅助功能权限时会静默无事件，排查热键问题先看权限。
- cpal Stream 非 Send；录音相关问题优先检查专属线程与 callback 是否只做轻量拷贝。
- macOS 实机 UI 截图可用 `screencapture -x -l <window-id>`；窗口 ID 用 `CGWindowListCopyWindowInfo` 查。
- 不要用全局鼠标点击自动化验证 UI，会干扰正在使用电脑的用户。

## 提交约定

- 使用 Conventional Commits：`feat(audio): …`、`fix(providers): …`、`docs: …`。
- scope 优先使用 06 章模块名：`hotkey` / `audio` / `inject` / `selection` / `providers` / `settings` / `history` / `orchestrator` / `app` / `platform`。
- 提交信息用中文。
- 产品名一律写作 **Typex**；bundle id 为 `ink.typex.app`。
