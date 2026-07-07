# 08 · 测试规范

> Typex 产品设计书 · 第八章
> [07 代码架构 §13](07-code-architecture.md) 是测试策略概览；本章是**可执行规范**：每类测试怎么写、写在哪、mock 什么、CI 怎么卡。
> 背景：本项目大量使用 AI 辅助编码。AI 产出代码的速度远超人工审查速度，**测试是唯一能规模化验收 AI 产出的手段**——因此测试不是「有空再补」，而是合入的前置条件。

---

## 1. 总原则

1. **测试与代码同 PR**。新增/修改行为的 PR 必须携带对应测试；只改文案/注释/纯样式除外。AI 生成的代码同样适用——让 AI 先写（或同时写）测试。
2. **测行为，不测实现**。断言公开接口的输入输出与副作用，不断言内部调用了什么函数——否则重构会大面积误报，AI 重构时尤其如此。
3. **可测性是架构约束的红利，不许绕过**：状态机是纯函数（[07 §5.2](07-code-architecture.md)）、service 之间不互相依赖、IO 全在 trait 后面——任何「这个没法测」都优先怀疑代码放错了层，而不是给测试开豁免。
4. **确定性**：单测/集成测不碰真实网络、真实时间、真实键盘麦克风。时间用注入的 `Clock` trait，网络用 wiremock，随机数固定种子。CI 上任何 flaky 测试按 bug 处理（修复或删除，不允许 retry 掩盖）。
5. **金字塔形状**：大量纯逻辑单测（毫秒级）→ 适量集成测（进程内，秒级）→ 少量人工端到端回归（发布前，见 §7）。v1 不上 GUI 自动化框架（键盘注入/权限弹窗的自动化成本远超收益，[07 §13](07-code-architecture.md) 已定）。

## 2. 覆盖率与门槛

- 工具：Rust 用 `cargo-llvm-cov`，前端用 vitest 内置 v8 coverage；CI 汇总到 PR 评论。
- **不设全局百分比红线**（百分比会诱导刷无意义断言），改为**分级要求**：

| 级别 | 模块 | 要求 |
|---|---|---|
| 关键 | `orchestrator/session.rs`、`providers/*`、`settings/migrate.rs`、`hotkey` 判定逻辑 | 行覆盖 ≥ 90%，且 §3 列出的场景清单逐条有具名测试 |
| 常规 | 其余 service、`shared/`、组件库 | 有代表性正反用例即可，覆盖率仅作参考 |
| 豁免 | `platform/`（OS API 胶水）、`app/`（纯转发）、窗口管理 | 不强制单测，靠 §7 人工回归覆盖 |

- 覆盖率只**监控下降**：关键模块覆盖率较主干下降 >2% 时 CI 警告（不 hard fail，允许有解释的合入）。

## 3. Rust 单元测试

位置：各模块内 `#[cfg(test)] mod tests`（纯逻辑）；跨模块的进程内测试放 `src-tauri/tests/`。

### 3.1 会话状态机（全项目最高优先级）

对 `advance(state, event) -> (state, Vec<Effect>)` 做穷举式表驱动测试。**必须逐条覆盖的场景清单**（与 [02 功能规格](02-features.md)、[05 §7](05-ux-spec.md) 对齐，新增行为先加进这张表）：

- 长按/短按：349ms 释放 = toggle 开始；351ms 释放 = push-to-talk 结束；toggle 模式下二次短按结束（三种模式语义一致，含助手模式）。
- 组合键：录音中追加第二触发键 → 模式切为翻译且音频保留；两键同时按下（乱序）等价。
- **组合键让路**：触发键按住期间出现普通键 down → 会话静默取消、Effect 含且仅含 `CancelRecording`（无任何注入/提示音）。
- 重按忽略：Transcribing/Processing/Injecting 中触发键按下 → 状态不变，Effect = `EmitUi(busy-hint)`；Failed 中按下 → 放弃旧会话开新录音。
- Esc：仅 Recording 态响应；其他态收到 Esc 不产生 Effect。
- 失败恢复：Transcribing 失败 → `recoverable_payload` 含音频；Processing 失败 → 含转写稿且 Effect 提供「复制原文」；重试从正确的 stage 恢复而非从头。
- **session_id 竞态**：携带旧 session_id 的 SttResult/LlmDelta 到达 → 被丢弃，状态零变化（这是防「上一句注入到下一句」的核心测试）。
- 整理层降级：听写中 Polish 超时/报错 → 注入原始转写 + `EmitUi(unpolished)`。
- 共享预整理：翻译/助手在 `polish_enabled=true` 时先使用文本整理提示词；`polish_enabled=false` 时跳过；整理超时/报错/未配置时直通原始转写继续下游。
- 翻译降级：STT 成功翻译失败 → Failed 且提供「注入原文」Effect。
- 取消后迟到的回调：Cancel 后 SttResult 到达 → 丢弃。
- 助手分流（F-3 / ADR-23）：Processing 中 `ProcessResult`（改写型）→ Injecting（直接替换选区）；`AssistantHandedOff`（回答型，已交回答弹窗流式）→ Idle + 释放音频；`ProcessFailed` → Failed（HUD 可重试/复制原文）。

### 3.2 其他纯逻辑单测

| 对象 | 重点用例 |
|---|---|
| hotkey 判定器（独立于 rdev 的纯逻辑层） | 修饰键 down/up 序列 → 触发事件流；AltGr 序列（Windows 上 AltGr = LCtrl+RAlt 连发）不误判为翻译组合；漏 release 后同一触发键 stale duplicate down 能重置并恢复触发 |
| VAD 切片（`audio/pipeline.rs`） | 构造合成 PCM（静音/正弦波拼接）：切点落在静音段、短音频不切、超长无静音音频的强制切片 |
| 重采样 | 44.1k/48k → 16k 的输出长度与频谱 sanity（正弦波频率不漂移） |
| `settings/migrate.rs` | 每个历史 schema_version 的样本 JSON（存 `tests/fixtures/settings/`）→ 迁移到最新版逐字段断言；未知字段保留；损坏 JSON → 回退默认并保留原文件为 `.bak` |
| `providers/error.rs` / `settings/migrate.rs` | HTTP 状态码 → ErrorCode 分类表；旧版 `keyring://` credentials 迁移清理 |
| 剪贴板恢复逻辑（`inject/paste.rs` 的纯逻辑部分） | 保存→注入→恢复的顺序；恢复失败不吞注入成功的结果 |
| PromptKit 模板渲染 | 变量替换（target_language、词典注入）、`ANSWER:` 前缀解析 |
| 助手改写/回答分流（`orchestrator/assistant.rs`） | 流首部前缀嗅探：`ANSWER:` 前缀（含前导空白、跨 chunk 切分）→ 回答型（呼出弹窗 + 流式）；无前缀 → 改写型静默收全文；无选区恒为回答型；空输出 → 错误 |

## 4. Rust 集成测试（`src-tauri/tests/`）

### 4.1 Provider × wiremock（每个 adapter 一个测试文件）

对 `openai_compat` / `volcengine` / `chat_completions` / `responses` 各建 wiremock 服务端，断言**请求构造**与**响应解析**两个方向：

- 请求：URL 拼接（base_url 带/不带尾斜杠）、鉴权头（Bearer vs 火山四件套 header）、multipart 字段完整性、自定义 extra_headers/extra_form 透传、超时设置生效。
- 响应：正常 JSON；SSE 流式（含 delta 分片跨 chunk 边界、`[DONE]`、Responses 的 `response.output_text.delta`/`response.failed` 事件）；火山 `X-Api-Status-Code` 非 20000000 的错误映射。
- 错误与重试：401 → `auth_error` 且**不重试**；429/503 → 退避重试 2 次后放弃；请求体在重试间不被消耗（multipart body 可重放）。
- 慢响应：mock 延迟 > timeout → `timeout` 分类。

**契约快照**：每个 adapter 把「构造出的完整 HTTP 请求」序列化为快照文件（`insta` crate）。厂商格式是外部契约，任何无意的请求变化都会在 diff 中显形——这对 AI 改代码尤其重要（AI 很容易「顺手优化」请求结构）。

### 4.2 服务级集成

- `SettingsService`：临时目录读写全流程、并发写、变更广播（watch 收到且仅收到一次）。
- `ProviderRegistry`：配置变更 → 只重建受影响的 profile 实例；缺少密钥或旧版 `keyring://` 引用 → 明确 `not_configured` 错误而非 panic。
- `history`：CRUD、保留期清理（注入 Clock 拨快 91 天）、并发写（WAL）。
- `InjectorChain`：用 mock Injector 断言后备链顺序、首个成功即停、全失败 → 剪贴板兜底 Effect。

### 4.3 显式不自动测（记录在案，靠 §7 人工回归）

真实键盘监听、真实注入到第三方应用、AX/UIA 读选区、麦克风权限弹窗、托盘/窗口行为、Wayland 各 compositor。理由：需要真实桌面会话与目标应用，CI 无法稳定复现；投入产出比见 §1.5。

## 5. 前端测试（vitest + @vue/test-utils）

### 5.1 IPC mock 基座（先建这个，其他测试都靠它）

`src/test/ipc-mock.ts`：mock 生成的 `bindings.ts`（command 返回可编程 fixture）+ 假 event 总线（测试内可 `emit("session://snapshot", …)`）。**所有组件测试通过它驱动，任何测试不得直接 mock `@tauri-apps/api`**——保证测试面向契约而非 Tauri 内部。

### 5.2 必测清单

| 对象 | 用例 |
|---|---|
| HUD（`Hud.vue`） | 表驱动：每种 `SessionSnapshot`（各 phase × 各 mode × 各错误码）→ 断言渲染的文案/按钮/徽标。这是把 [05 §3](05-ux-spec.md) 的状态表固化成测试 |
| 错误码 → 文案 | 遍历 Rust 导出的全部 ErrorCode（从 bindings 类型取）：zh-CN 与 en 均有对应 i18n key（防「Rust 加了错误码忘了文案」——**这条是编译期抓不到的契约缝隙，必须测**） |
| HotkeyRecorder | 录制态进出、组合键渲染、冲突警告展示 |
| ProviderCard 表单 | 按 kind 动态渲染字段（openai_compat vs volcengine 双凭据）；密钥字段不回显明文；「测试」按钮三态（loading/成功延迟/分类错误） |
| 回答弹窗 | `started` 重置内容 + 指令回显；流式 delta 追加渲染；Markdown sanitize（`<script>`、raw HTML 注入被清洗——LLM 输出是不可信输入，**这是安全测试**） |
| stores | settings patch 乐观更新与回滚；session store 严格镜像 event（不自行推导状态） |
| shared/ 工具 | 常规单测 |

### 5.3 视觉与主题

v1 不上视觉回归截图工具；替代措施：tokens.css 的 lint（禁止组件硬编码色值）+ 双主题的人工走查项（§7 清单内）。v1.x 若组件库变大再评估 Storybook + 截图对比。

## 6. 提示词回归评测（LLM 相关的特殊层）

提示词（整理/翻译/处理判定）的行为无法用普通断言穷尽，但**不测就等于每次改提示词都在裸奔**：

- 语料：`docs/fixtures/denoise-cases.md`（中英各 ≥10 例/类：语气词、改口、重复、格式指令、专有名词保留）+ `translate-cases.md` + `rewrite-vs-answer-cases.md`（F-3a 判定）。每例含输入与**期望要点**（如「不得出现『嗯』」「必须保留 Typex 一词」），而非逐字期望输出。
- 执行：`scripts/eval-prompts.ts`——读语料 → 调真实 API（需环境变量密钥）→ 按要点做程序化断言（正则/包含/长度比）+ 可选 LLM-as-judge 打分 → 输出通过率报告。
- 节奏：**不进 PR CI**（成本与波动），而是：改动提示词的 PR 必须附本地评测报告；每个里程碑结束跑全量并存档比较。
- 语料本身随 bug 增长：线上发现的整理翻车案例，修复时先加进语料。

## 7. 人工回归清单（发布门槛）

`docs/release-checklist.md`（M1 时建立，随功能追加）按 [02 章各功能验收标准](02-features.md) 展开为逐条可勾选项，**三平台各过一遍**（macOS / Windows / Ubuntu X11 必过；KDE Wayland 尽力）。结构：

1. 安装与首启（onboarding 全流程、权限引导、跳过路径）
2. F-1/F-2/F-3 验收标准逐条
3. 注入矩阵：微信/Slack/VS Code/浏览器/终端 × 中英文
4. 错误注入：断网、错密钥、慢网（系统级限速）、无麦克风权限
5. 深浅主题走查 + reduced-motion
6. 更新：从上一版本升级、设置迁移
7. 资源：空闲内存/CPU 实测对照 [07 §12 预算](07-code-architecture.md)

每次 release 的勾选结果存 `docs/releases/<version>-checklist.md`，作为发布记录。

## 8. CI 流水线（`.github/workflows/ci.yml`）

```
PR / 主干 push：
├── lint:      rustfmt --check · clippy -D warnings · eslint · stylelint · vue-tsc
├── gen-check: 重新生成 ipc bindings + 与提交内容 diff（契约新鲜度）
├── test-rust: cargo test（单测+集成测，Linux 跑全量；mac/win 跑平台相关子集）
├── test-web:  vitest --coverage
├── coverage:  llvm-cov 汇总 → PR 评论（关键模块下降 >2% 警告）
├── build:     三平台 tauri build（PR 上仅 Linux 快检，主干全矩阵）
└── size:      hud chunk 体积断言 · 安装包体积对照预算
```

- 任何一步红 = 不可合并；flaky 测试当天处理（§1.4）。
- `cargo audit` / `npm audit` 每周定时任务 + release 前强制。

## 9. AI 辅助开发的专项纪律

1. **先契约后实现**：让 AI 写新功能时，先产出（或先审）测试与类型签名，再产出实现——测试是给 AI 的验收标准，防止「实现顺便重新定义了需求」。
2. **快照防漂移**：外部契约（HTTP 请求形状、IPC 载荷、settings schema）全部有快照/fixture 测试，AI 的「顺手重构」会立刻在 diff 里显形（§4.1）。
3. **场景清单是需求的镜像**：§3.1 的状态机场景表与 02/05 章一一对应；AI 修改行为时，要求它先更新场景表再改代码——表里没有的行为不存在。
4. **禁止 AI 弱化测试来转绿**：CI 失败时，修改断言/删除用例/加 `#[ignore]` 的 diff 必须在 PR 描述中单独说明理由；review 时对「测试变松」保持最高警惕。
5. 生成的测试也要 review：重点看断言是否真的约束行为（空断言、tautology 断言是 AI 常见产物）。

## 10. 落地顺序

- **M0**：CI 骨架（lint + test + gen-check）先于一切功能代码；状态机第一批场景测试与状态机本体同 PR。
- **M1**：wiremock provider 套件、settings 迁移 fixture、前端 IPC mock 基座 + HUD 表驱动测试、denoise 语料 v1、release-checklist v1。
- **M2–M4**：随功能补充各自场景；M4 补 Wayland 人工清单项。
- **M5**：全量人工回归 + 审计任务，按 §7/§8 执行。
