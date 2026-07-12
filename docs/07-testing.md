# 07 · 测试规范

> Typex 产品设计书 · 第七章
> [06 代码架构 §13](06-code-architecture.md) 是测试策略概览；本章是**可执行规范**：每类测试怎么写、写在哪、mock 什么、CI 怎么卡。
> 背景：本项目大量使用 AI 辅助编码。AI 产出代码的速度远超人工审查速度，**测试是唯一能规模化验收 AI 产出的手段**——因此测试不是「有空再补」，而是合入的前置条件。

---

## 1. 总原则

1. **测试与代码同 PR**。新增/修改行为的 PR 必须携带对应测试；只改文案/注释/纯样式除外。AI 生成的代码同样适用——让 AI 先写（或同时写）测试。
2. **测行为，不测实现**。断言公开接口的输入输出与副作用，不断言内部调用了什么函数——否则重构会大面积误报，AI 重构时尤其如此。
3. **可测性是架构约束的红利，不许绕过**：状态机是纯函数（[06 §5.2](06-code-architecture.md)）、service 之间不互相依赖、IO 全在 trait 后面——任何「这个没法测」都优先怀疑代码放错了层，而不是给测试开豁免。
4. **确定性**：单测/集成测不碰真实网络、真实时间、真实键盘麦克风。时间用注入的 `Clock` trait，网络用 wiremock，随机数固定种子。CI 上任何 flaky 测试按 bug 处理（修复或删除，不允许 retry 掩盖）。
5. **金字塔形状**：大量纯逻辑单测（毫秒级）→ 适量集成测（进程内，秒级）→ 少量人工端到端回归（发布前，见 §7）。当前不上 GUI 自动化框架（键盘注入/权限弹窗的自动化成本远超收益，[06 §13](06-code-architecture.md) 已定）。

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

- 长按/短按：349ms 释放 = toggle 开始；351ms 释放 = push-to-talk 结束；toggle 模式下听写/翻译在第二次 keydown 立即结束，助手模式等待完整 chord 释放后结束（确保读取选区时触发修饰键已松开）。
- 组合键：录音中追加第二触发键 → 模式切为翻译且音频保留；两键同时按下（乱序）等价。
- **组合键让路**：触发键按住期间出现普通键 down → 会话静默取消，Effect 仅含取消/释放录音（`CancelRecording` + `ReleaseAudio`），无 `EmitUi`、注入或提示音。
- 暂停：Recording 的 push-to-talk 与短按 toggle 两种状态收到托盘暂停发来的 `Cancel` → Idle + 释放录音；Transcribing/Processing 不被暂停强杀。
- 重按忽略：Transcribing/Processing/Injecting 中触发键按下 → 状态不变，Effect = `EmitUi(busy-hint)`；Failed 中按下 → 放弃旧会话开新录音。
- Esc：仅 Recording 态响应；其他态收到 Esc 不产生 Effect。
- 失败恢复：Transcribing 失败 → `recoverable_payload` 含音频；Processing 失败 → 含转写稿且 Effect 提供「复制原文」；重试从正确的 stage 恢复而非从头。
- 录音失败：Recording 中收到启动失败或运行时设备拔出 → `Failed(Recording)`，Effect 含 `CancelRecording` + 错误反馈；Retry → 同一模式重新 `StartRecording`，不得进入 STT/Injecting。
- **session_id 竞态**：携带旧 session_id 的 SttResult/LlmDelta 到达 → 被丢弃，状态零变化（这是防「上一句注入到下一句」的核心测试）。
- 整理层降级：听写中 Polish 超时/报错 → 注入原始转写 + `EmitUi(unpolished)`。
- 共享预整理：翻译/助手在 `polish_enabled=true` 时先使用文本整理提示词；`polish_enabled=false` 时跳过；整理超时/报错/未配置时直通原始转写继续下游。
- 翻译降级：STT 成功翻译失败 → Failed 且提供「注入原文」Effect。
- 取消后迟到的回调：Cancel 后 SttResult 到达 → 丢弃。
- 助手分流（F-3 / ADR-23）：Processing 中 `ProcessResult`（改写型）→ Injecting（直接替换选区）；`AssistantHandedOff`（回答型，已交回答弹窗流式）→ Idle + 释放音频；`ProcessFailed` → Failed（HUD 可重试/复制原文）。

### 3.2 其他纯逻辑单测

| 对象 | 重点用例 |
|---|---|
| hotkey 判定器（独立于 OS backend 的纯逻辑层） | `KeyId` 别名归一化表（Enter/Return、Digit/Num、Arrow/LeftArrow、AltRight/AltGr、Meta/Win、Menu、标点、字母数字、F13–F19、Numpad/Kp）；多键 chord 的 partial/完整/乱序/共享非子集/翻译并集，相同、空或任一子集配置在前后端均拒绝；partial 全释放无事件、已激活手势等全部 tracked 键释放才 Up；active single/multi chord 配置热更新先 Up、旧 release 不重复，partial 更新无事件，相同配置与无关 settings 更新完全幂等；普通键让路；rdev 暂停 transition 清空 held；修饰键 down/up 与 349/351ms 边界；Windows scan code 物理位置不随布局 VK 漂移；75 ms 确认窗内 Right Ctrl+C、物理 RAlt+普通键和 AltGr 伪 LCtrl+RAlt 均无语义事件/副作用，单键确认 ≤100 ms，快速释放保留原始 held_ms；`LLKHF_INJECTED` 被忽略；仅已确认助手手势吞 RAlt keyup，配置更新后对应旧 RAlt keyup 仍恰好吞一次；callback terminal Failed 后 raw event 零产出且 `WM_QUIT` 不覆盖失败；漏 release 后 stale duplicate down 重置恢复，普通键 auto-repeat 不得误判 stale release |
| Windows 候选录音 adapter | 原始触发键立即发候选 token；75 ms 确认携带同 token 提升；Ctrl+C/物理 RAlt+普通键/AltGr 匹配取消且无可见副作用；双键翻译复用候选；快速释放保留原始 held_ms；暂停、配置更新、hook 失败/意外终止与退出清理未决 token |
| Windows hook health monitor | Healthy/Starting 与暂停态不误取消；运行期 Failed/意外 Stopped 对 push-to-talk/toggle 统一发一次 `Cancel`；重复终态不重复；启动失败使用同一可订阅状态；主动 Shutdown 静默 |
| Windows 音频转换与设备解析 | WASAPI 常见 `f32/i16/u16` → mono f32 的边界值、声道混合、重采样长度；有界缓冲溢出计数；endpoint ID 精确选择；旧 display name 唯一匹配迁移；同名歧义/固定设备缺失；设备拔出/stream error 脱敏分类与主动通知 |
| Windows 坐标与完整性纯逻辑 | mixed-DPI、负坐标与 work area 转换；目标完整性高于 Typex 时判定 UIPI 降级，不触发自动提权 |
| VAD 与切片（`audio/vad.rs` / `audio/pipeline.rs`） | schema v7 迁移与门限校验；能量/神经网络两条路径；Silero 初始化/推理失败降级；弱声连续 90 ms 保底与纯静音拒绝；首部 300 ms/尾部 150 ms 非对称 padding；长录音切片沿用录音快照；短音频不切、超长无静音音频强制切片 |
| 重采样 | 44.1k/48k → 16k 的输出长度与频谱 sanity（正弦波频率不漂移） |
| `settings/migrate.rs` | 每个历史 schema_version 的样本 JSON（存 `tests/fixtures/settings/`）→ 迁移到最新版逐字段断言；未知字段保留；损坏 JSON → 回退默认并保留原文件为 `.bak` |
| `providers/error.rs` / `settings/migrate.rs` | HTTP 状态码 → ErrorCode 分类表；旧版 `keyring://` credentials 迁移清理 |
| 本地模型清单/导入 | 内置清单 + 用户清单合并；导入 LLM GGUF / llama ASR GGUF+mmproj / sherpa ONNX+tokens；导入模型删除同步清理用户清单；零配置兜底只选内置已下载模型；内置清单包含高配手动模型（Whisper large-v3、Qwen3 14B/30B-A3B/32B）且不进入自动硬件分档 |
| 本地 llama GPU→CPU fallback | load mode；CPU 模型 `n_gpu_layers=0` 且 devices 为空、context 的 K/Q/V 与 op offload 关闭、ASR mtmd `use_gpu=false`；仅 GPU runtime 错误且 LLM 首个可见 delta 前重试一次；输入错误、CPU mode、已输出与第二次错误不重试；ASR 非流式整次重试；并发请求由 inference lease 串行且只加载一个共享 CPU 条目、fallback 前释放失败 GPU 条目、CPU 加载不持缓存锁、显式 unload 后 detached fallback 不回填、`UnloadAfterUse` 只清自己的缓存代际 |
| 剪贴板事务与 Windows 空选区判定（`inject/paste.rs` / `inject/windows.rs` 的纯逻辑部分） | 保存→注入→恢复的顺序；恢复失败不吞注入成功的结果；原多格式快照完整传递；用户/剪贴板管理器中途改变 sequence/owner 时不覆盖；同一非空 owner 分批发布格式导致 sequence 连续变化时接受最终原子载荷并更新恢复基线；复制文本与旧值相同仍按 sequence 识别；sequence 相同与 `u32` 回绕边界；`vscode-editor-data` 缺失、合法 JSON true/false、损坏/字段类型错误/超大 metadata、尾随 NUL 与 Unicode 文本。只有可可靠证明的空选区才返回 `None`，未知情况保留文本 |
| PromptKit 模板渲染 | 变量替换（target_language、词典注入）、`ANSWER:` 前缀解析 |
| F-10 词典 | settings 迁移与规范化；STT prompt / 火山 corpus / 本地 hotwords 注入；空词典时 LLM `<dictionary>` 行省略 |
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
- `AudioService`：候选 `prepare/promote/cancel` 的 token 隔离；打开中取消、确认先于 stream ready、启动失败延迟到确认、运行时失败与取消竞态；候选期间无电平输出且取消立即释放。
- `ProviderRegistry`：配置变更 → 只重建受影响的 profile 实例；缺少密钥或旧版 `keyring://` 引用 → 明确 `not_configured` 错误而非 panic。
- `history`：CRUD、保留期清理（注入 Clock 拨快 91 天）、并发写（WAL）。
- `InjectorChain`：用 mock Injector 断言后备链顺序、首个成功即停、全失败 → 剪贴板兜底 Effect。
- Windows SendInput adapter：用构造器测试 Ctrl+V、BMP、surrogate pair、emoji、换行和每个 keyup 的 INPUT 序列；系统调用本身由受控 harness 回归。
- Windows UIA adapter：可注入 worker 通道与纯结果归并层覆盖 `Supported`（空/非空）、`Unsupported`、COM/Internal 错误、超时后熔断、请求队列 full/stopped、响应通道断开，以及多 range 文本与 bounds 并集；UIA 明确返回空选区时不得误走 Ctrl+C。

### 4.3 Windows 受控系统契约 harness

`src-tauri/tests/windows_platform_harness.rs` 是 `#[ignore]` 的交互桌面契约测试，不随普通 `cargo test` 自动执行。它使用 RichEdit 可编辑控件、经典只读 Edit、无 TextPattern 的 Button 后备 fixture 和普通 Win32 HUD fixture，验证 UIA 选区/bounds、Ctrl+C 剪贴板事务、Unicode/emoji、Ctrl+V、错目标拒绝、只读拒绝，以及 `WS_EX_NOACTIVATE`/HUD 显示前后台 HWND 不变。显式串行运行：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test windows_platform_harness -- --ignored --test-threads=1
```

该 harness 会使用真实 UIA、SendInput 和系统剪贴板，必须在已解锁的交互式 Windows 桌面运行；剪贴板内容由产品事务恢复，测试和日志不得输出其内容。真实第三方应用只作代表性人工抽测。

### 4.4 显式不自动测（记录在案，靠 §7 人工回归）

物理键盘布局、真实麦克风权限弹窗、任意第三方应用兼容性、混合 DPI 多显示器、睡眠/RDP、托盘像素观感与 Wayland 各 compositor 不自动测。Windows 跨完整性级别的真实 UIPI 拒绝需要另一个管理员权限目标进程，不能由同进程 harness 稳定构造；发布前人工验证，可选使用单独 elevated fixture 辅助。`uipi_blocks_injection` 的完整性级别比较仍必须由纯逻辑测试自动覆盖，人工项不能替代该门禁。可控的 Windows 原始事件解码、SendInput 序列、UIA adapter 和坐标换算同样必须由纯逻辑测试或 §4.3 harness 覆盖。

## 5. 前端测试（vitest + @vue/test-utils）

### 5.1 IPC mock 基座（先建这个，其他测试都靠它）

`src/test/ipc-mock.ts`：mock 生成的 `bindings.ts`（command 返回可编程 fixture）+ 假 event 总线（测试内可 `emit("session://snapshot", …)`）。**所有组件测试通过它驱动，任何测试不得直接 mock `@tauri-apps/api`**——保证测试面向契约而非 Tauri 内部。

### 5.2 必测清单

| 对象 | 用例 |
|---|---|
| HUD（`Hud.vue`） | 表驱动：每种 `SessionSnapshot`（各 phase × 各 mode × 各错误码）→ 断言渲染的文案/按钮/徽标。这是把 [05 §3](05-ux-spec.md) 的状态表固化成测试 |
| 错误码 → 文案 | 遍历 Rust 导出的全部 ErrorCode（从 bindings 类型取）：zh-CN 与 en 均有对应 i18n key（防「Rust 加了错误码忘了文案」——**这条是编译期抓不到的契约缝隙，必须测**） |
| HotkeyRecorder | 录制态只切换按钮文案且不展开提示块、浏览器 code→稳定 `KeyId` 表、Windows WebView2 误报 `ShiftLeft` 但 `location=RIGHT` 时仍保存 `ShiftRight`、完整组合键保存/渲染、Esc 静默取消、历史别名展示、平台化标签与冲突警告 |
| 首次启动引导 | 第 4 步可直接录制听写/助手快捷键；修改立即持久化并同步派生翻译组合与练习提示；相同、空或任一包含另一组合时阻止保存并显示校验提示 |
| 首次启动完成 | `onboarding_done` 与自启选择必须先保存，再调用 `complete_onboarding`；提交中不可重复触发；主页切换失败时保留引导页并显示可重试错误 |
| 模型管理 | `hardware_ok=false && downloadable=true` 时显示低于建议但下载按钮可用并调用下载；`downloadable=false` 时仍禁用 |
| ProviderCard 表单 | 按 kind 动态渲染字段（openai_compat vs volcengine 双凭据）；密钥字段不回显明文；「测试」按钮三态（loading/成功延迟/分类错误） |
| 回答弹窗 | `started` 重置内容 + 指令回显；流式 delta 追加渲染；Markdown sanitize（`<script>`、raw HTML 注入被清洗——LLM 输出是不可信输入，**这是安全测试**） |
| stores | settings patch 乐观更新与回滚；session store 严格镜像 event（不自行推导状态） |
| 听写设置 | VAD 分段控件键盘切换；只渲染当前模式门限；两组范围/步长/显示精度；切换模式保留两组独立值并持久化 |
| shared/ 工具 | 常规单测 |

### 5.3 视觉与主题

当前不上视觉回归截图工具；替代措施：tokens.css 的 lint（禁止组件硬编码色值）+ 双主题的人工走查项（§7 清单内）。若组件库变大再评估 Storybook + 截图对比。

## 6. 提示词回归评测（LLM 相关的特殊层）

提示词（整理/翻译/处理判定）的行为无法用普通断言穷尽，但**不测就等于每次改提示词都在裸奔**：

- 语料：`docs/fixtures/denoise-cases.md`（中英各 ≥10 例/类：语气词、改口、重复、格式指令、专有名词保留）+ `translate-cases.md` + `rewrite-vs-answer-cases.md`（F-3a 判定）。每例含输入与**期望要点**（如「不得出现『嗯』」「必须保留 Typex 一词」），而非逐字期望输出。
- 执行：当前仅维护语料与期望要点；提示词改动时用运行时提示词对照语料做本地评测。若后续恢复自动评测脚本，脚本必须复用 `providers/llm/prompt.rs` 与前端默认模板，不得维护单独提示词副本。
- 节奏：**不进 PR CI**（成本与波动），而是：改动提示词的 PR 必须附本地评测报告；重要版本发布前跑全量并存档比较。
- 语料本身随 bug 增长：线上发现的整理翻车案例，修复时先加进语料。

## 7. 人工回归清单（发布门槛）

`docs/09-release-checklist.md` 随功能追加，按 [02 章各功能验收标准](02-features.md) 展开为逐条可勾选项。当前 macOS 与 Windows x64 必过；Ubuntu X11 / KDE Wayland 随对应平台后端适配推进纳入回归范围。结构：

1. 安装与首启（onboarding 全流程、权限引导、跳过路径）
2. F-1/F-2/F-3 验收标准逐条
3. 注入矩阵：受控 harness + 平台代表应用（Windows 为记事本/Edge/VS Code/Windows Terminal）× 中英文；微信/Slack/Word 等非阻塞抽测
4. 错误注入：断网、错密钥、慢网（系统级限速）、无麦克风权限
5. 深浅主题走查 + reduced-motion
6. 更新：从上一版本升级、设置迁移
7. 资源：空闲内存/CPU 实测对照 [06 §12 预算](06-code-architecture.md)
8. 起音与让路：Windows 按下后立即发音、轻声起音、Ctrl+C、AltGr、纯静音和长录音；确认普通组合键无 HUD/提示音/电平/历史/Provider 副作用

本清单长期维护，发版时人工参考执行，不要求逐版本复制归档。

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
- Windows PR job 必须运行 Rust default 与 `--no-default-features` 两套 check/clippy/test、`pnpm gen:ipc`/build/test；Windows 自启纯逻辑单测覆盖空格路径加引号、旧 debug/安装路径修复、正确路径不写和关闭时删除残留。另构建 debug EXE 并验证为 GUI subsystem。NSIS smoke 在安装器配置变更或主干构建中运行，检查默认目录 hook 已进入渲染脚本，并解包验证 sherpa/ONNX、VC++ runtime、Vulkan loader 和 runtime manifest 位于 EXE 同目录，第三方许可位于 `licenses/`，且文件名、相对路径与哈希一致；release 与包内 EXE 均须为 GUI subsystem。内嵌的 WebView2 Evergreen Bootstrapper 必须具有有效 Microsoft Authenticode，PE import 检查必须证明默认 feature 不依赖安装目录外的非系统 DLL。Tauri 2 Windows updater 直接复用同一 NSIS `.exe`，构建验证和 publish job 下载 artifact 后都必须用 `TAURI_UPDATER_PUBKEY` 对对应 `.exe.sig` 实际验签，不能只检查签名文件存在或非空；macOS updater archive 同样验签。平台 artifact 汇总必须先拒绝重复资产名，再生成唯一 manifest。CI/测试 artifact 不执行发布。
- `cargo audit` / `npm audit` 每周定时任务 + release 前强制。

## 9. AI 辅助开发的专项纪律

1. **先契约后实现**：让 AI 写新功能时，先产出（或先审）测试与类型签名，再产出实现——测试是给 AI 的验收标准，防止「实现顺便重新定义了需求」。
2. **快照防漂移**：外部契约（HTTP 请求形状、IPC 载荷、settings schema）全部有快照/fixture 测试，AI 的「顺手重构」会立刻在 diff 里显形（§4.1）。
3. **场景清单是需求的镜像**：§3.1 的状态机场景表与 02/05 章一一对应；AI 修改行为时，要求它先更新场景表再改代码——表里没有的行为不存在。
4. **禁止 AI 弱化测试来转绿**：CI 失败时，修改断言/删除用例/加 `#[ignore]` 的 diff 必须在 PR 描述中单独说明理由；review 时对「测试变松」保持最高警惕。
5. 生成的测试也要 review：重点看断言是否真的约束行为（空断言、tautology 断言是 AI 常见产物）。

## 10. 维护节奏

- CI 骨架（lint + test + gen-check）先于功能扩展保持可用；关键状态机场景测试与状态机变更同 PR。
- 新增或修改功能时，同步补充场景表、自动化测试、必要的人工清单项。
- 平台后端新增或能力边界变化时，同步更新 §7 人工回归范围与 [09 发布人工回归清单](09-release-checklist.md)。
- 提示词行为变更时，同步更新 `fixtures/`，并附本地评测报告。
