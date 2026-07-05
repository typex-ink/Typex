# Typex 实现路线表

> 依据设计书 [`docs/`](docs/)（01–09 章 + mockups）生成。
> 每完成一个 checkpoint：将 `[ ]` 改为 `[x]`、附完成日期，并提交一次 git commit。
> 里程碑划分对应设计书 [06 章](docs/06-roadmap.md)；模块归属对应 [07 章](docs/07-code-architecture.md)；UI 以 [`docs/mockups/ui-mono.html`](docs/mockups/ui-mono.html) + `tokens.css` 为准。
> 平台优先级：macOS 先行（当前开发机），代码结构保持三平台可扩展。

---

## M0 · 骨架（管线端到端打通）

- [x] **CP-0.1 工程初始化**（2026-07-05）
  Tauri 2 工程：Vue 3 + TS + Tailwind CSS v4 + Pinia + Vite 多窗口入口（hud/assistant/settings/onboarding/home）；`src/styles/tokens.css` 从 mockup 复制落地；`src-tauri` 骨架 + `tauri.conf.json`（appId `ink.typex.app`）；GPLv3 LICENSE；`cargo check` + `pnpm build` 通过。
- [x] **CP-0.2 Rust 模块骨架 + IPC 契约链路**（2026-07-05）
  按 07 §4 目录树建全部模块（types/error/settings schema 数据先行）；tauri-specta 生成 `src/ipc/bindings.ts` 链路跑通；`error.rs` ErrorCode 全集定义。
- [x] **CP-0.3 托盘 + 单实例 + 设置存储**（2026-07-05）
  托盘图标（五柱波形 glyph，macOS template image）+ 基础菜单（设置/暂停/退出）；tauri-plugin-single-instance；SettingsService（JSON 读写 + schema_version + watch 广播）。
- [x] **CP-0.4 全局按键监听（rdev）**（2026-07-05）
  rdev 独立线程监听右 ⌘（macOS）按住/松开事件 → mpsc 发往主循环；长短按 350ms 判定的纯逻辑判定器 + 单测；组合键让路规则（普通键 down → 取消）。
- [x] **CP-0.5 录音管线**（2026-07-05）
  cpal 以设备原生采样率开流（callback 只写 ring buffer）→ worker 线程 rubato 重采样 16 kHz mono → hound 编 WAV；电平 RMS 计算（50ms 节流事件）。
- [x] **CP-0.6 STT + 注入**（2026-07-05）
  `SttProvider` trait + `openai_compat` 实现（multipart /audio/transcriptions，reqwest）；`Injector` trait + `paste` 后端（arboard 保存剪贴板 → 写入 → enigo Cmd+V → 恢复）。
- [ ] **CP-0.7 M0 端到端验收**（⚠ 需实机人工验证：麦克风说话 + 真实 STT 密钥，代码链路已就绪）
  按住右 ⌘ 说话 → 松开 → STT（写死/环境变量配置）→ 文字注入光标处。全链路在 macOS 实机跑通。

## M1 · 听写可用（F-1 + F-9 完整体验）

- [x] **CP-1.1 会话状态机（纯函数）**（2026-07-05）
  `orchestrator/session.rs`：`advance(state, event) -> (state, Vec<Effect>)` 转移表；08 §3.1 场景清单逐条具名单测（长短按/组合让路/重按忽略/Esc/失败恢复/session_id 竞态/取消后迟到回调/整理降级/翻译降级）。
- [x] **CP-1.2 Orchestrator 执行器**（2026-07-05）
  Effect dispatch 到各 service；session_id 自增与回调校验；`session://snapshot` 事件推送；M0 临时串联代码替换掉。
- [x] **CP-1.3 HUD 窗口**（2026-07-05）
  常驻隐藏式无边框置顶胶囊窗（macOS NSPanel nonactivating，tauri-nspanel）；底部居中距底边 48px、高 44px；全部状态渲染（录音+波形 Canvas/翻译徽标/处理中呼吸/成功回弹/失败+重试+复制原文/原样标注/无声提示）——严格对照 mockup §3；出现 220ms spring / 消失 160ms；`prefers-reduced-motion` 降级。
- [x] **CP-1.4 LlmProvider**（2026-07-05）
  trait + `chat_completions` + `responses` 两个 adapter（SSE 流式解析）；错误分类映射 + 指数退避重试（http.rs 统一）；wiremock 集成测试（请求构造/SSE 跨 chunk/错误分类/401 不重试/429 重试）。
- [x] **CP-1.5 F-9 文本整理**（2026-07-05）
  PromptKit 模板渲染（占位符校验/可选段省略）+ 内置整理提示词（03 §3.4 逐字）；整理失败自动降级原始转写 + HUD「未整理」标注；原样模式开关；`docs/fixtures/denoise-cases.md` 测试样例集。
- [x] **CP-1.6 Profile / 槽位 / 密钥系统**（2026-07-05）
  settings schema（slots + profiles[]，03 §6 形态）；keyring 密钥存取（`keyring://` 引用解析，明文不落盘）；ProviderRegistry（配置变更惰性重建）；LLM 三槽共用连接规则；profile CRUD + activate + test 的 IPC command 全套。
- [x] **CP-1.7 设置窗口**（2026-07-05）
  720×520 左导航 9 页框架；组件库（Button/Input/SecretInput/Select/Toggle/Kbd/ProviderCard/Callout/HotkeyRecorder…全部对照 mockup §7 组件全态）；通用页 / 听写页（整理开关+提示词模板编辑器+注入方式+麦克风选择+电平预览）/ 模型服务页（ProviderCard+编辑子页+预设模板+测试连接+切换▾多档案）/ 快捷键页（HotkeyRecorder+冲突警告）；深浅双主题。
- [x] **CP-1.8 Onboarding 5 步**（2026-07-05）
  640×480 向导：欢迎（图标+口号+左下语言切换）/ 权限（macOS 麦克风+辅助功能+输入监听实时轮询）/ 模型（云端直填两组表单）/ 快捷键（默认键位+练习输入框）/ 完成；跳过路径。
- [x] **CP-1.9 VAD + 长录音 + 提示音**（2026-07-05）
  Silero VAD（vad-rs）首尾静音裁剪；超限自动切片（静音边界）分段转写拼接；HUD 录制时长显示 + 10 分钟温和提示；rodio 提示音三枚（开始/成功/错误，可关）。
- [x] **CP-1.10 i18n + 错误文案**（2026-07-05）
  vue-i18n zh-CN/en 全量资源；ErrorCode → 文案映射全表（05 §9）；「Rust 错误码都有 i18n key」的 vitest 契约测试。
- [x] **CP-1.11 M1 验收**（2026-07-05）
  F-1 验收标准逐条自查（注入矩阵人工项除外）；`cargo test` + `pnpm test` 全绿。

## M2 · 翻译（F-2）

- [x] **CP-2.1 翻译触发**（2026-07-05）
  右 ⌘ + 右 ⌥ 组合判定（乐观启动、350ms 窗口内组合无缝切翻译、录音中追加升级）；判定器单测（含 AltGr 序列不误判）。
- [x] **CP-2.2 翻译流水线**（2026-07-05）
  翻译提示词（03 §3.4，双向判向并入提示词）；流式接收一次性注入；STT 成功翻译失败 → HUD「注入原文」降级。
- [x] **CP-2.3 翻译 UI**（2026-07-05）
  HUD `中 → EN` 徽标 + 点击语言快切；设置-翻译页（源/目标语言、双向开关、提示词编辑器）；托盘「翻译目标 ▸」子菜单。
- [x] **CP-2.4 M2 验收**（2026-07-06：规格逐条自查通过 · 双向开关原不生效已修——双向子句改用 `{bidirectional_*}` 可选占位符，关闭即整行省略（含单测）· 双向样句集 `docs/fixtures/translate-cases.md`（中↔EN 各 20 句 + 结构保留 4 例）已备，真实 API 评测随 CP-6.10 脚本执行；P50 延迟为真机人工项留发布回归）
  F-2 验收标准自查；双向样句测试。

## M3 · 文本处理与问答（F-3）

- [x] **CP-3.1 选中文本读取**（2026-07-05）
  `SelectionReader` trait + macOS AX API 主路径 + Cmd+C 剪贴板降级（恢复剪贴板、300ms 超时、误触发检测）。
- [x] **CP-3.2 助手面板**（2026-07-05）
  560px 浮窗（上 1/3 居中、圆角 16、失焦隐藏、📌 固定）；上下文芯片（来源+字数+移除）；输入行（文本+按住语音）；流式 Markdown 渲染（markdown-it + sanitize，30fps 节流）；动作行（替换选区/插入光标/复制）——对照 mockup §5。
- [x] **CP-3.3 F-3a 原地处理**（2026-07-05）
  处理提示词 + `ANSWER:` 判定（改写→替换选区；回答→仅展示）；自动/预览确认两种处置；读不到选区降级为 F-3b。
- [x] **CP-3.4 F-3b 单轮问答**（2026-07-05）
  无选区呼出（助手键短按=面板，按住=面板+录音）；单轮语义（不携带历史）；托盘「打开助手」；设置-助手页。
- [x] **CP-3.6 助手交互重构（ADR-23）**（2026-07-06）
  面板降级为只读回答弹窗（指令回显 + 流式回答 + ✕，失焦即关，无输入条/动作行/📌）；改写型不弹窗直接替换选区、反馈走 HUD 完整流水线（`ANSWER:` 流首部嗅探分流）；助手键短按=切换录音；移除托盘「打开助手」、打字提问 command、结果处置设置。
- [x] **CP-3.5 M3 验收**（2026-07-06：验收标准 1–5/7 逐条对照代码与单测通过（assistant.rs 分流 9 例单测 · 单轮无历史构造 · request_id 过滤旧流 · Started 事件重置弹窗 · 失焦自动关闭 · 读选区失败降级 F-3b）；判定样例集 `docs/fixtures/rewrite-vs-answer-cases.md`（改写 10 + 提问 10 + 边界 5）已备，真实 API 评测随 CP-6.10；首 token 延迟为真机人工项）
  F-3 验收标准自查（连续提问无泄漏、改写 vs 回答判定样例）。

## M4 · 历史 / 主页 / 完善

- [x] **CP-4.1 历史记录（F-7）**（2026-07-05）
  rusqlite WAL；每条记录含模式/原始转写/结果/目标应用/时长/字数；保留期清理；设置-历史页（开关/期限/清空）；关闭历史仍保「最近 1 条」内存级。
- [x] **CP-4.2 主页窗口**（2026-07-05）
  880×560 侧边栏（品牌行 mini 图标/首页/历史记录/底部 设置+◐ 主题切换）；首页页签（大标题+快捷键提示+统计卡组 2×2+最近 3 条+空状态）；历史页签（搜索/展开双栏对照/复制/删除/清空）——对照 mockup §1。
- [x] **CP-4.3 托盘完整菜单**（2026-07-05）
  状态行（就绪/录音中/处理中/暂停/错误）+ 打开助手/复制上次结果/文本整理开关/翻译目标▸/模型▸（三组档案切换）/暂停/设置/主页/检查更新/退出；托盘图标状态动画（录音红点角标/处理呼吸）。
- [x] **CP-4.4 诊断页 + 权限**（2026-07-05）
  环境自检（权限状态/注入后端/compositor 探测，✓/✗+修复引导）；日志（tracing + redact 层：密钥与转写文本不入日志）+ 导出诊断包。
- [x] **CP-4.5 打磨**（2026-07-05）
  深浅主题全界面走查（对照 mockup 双主题）；动效规格核对（04 §6）；reduced-motion；应用图标（typex.svg 波形成 T → icns/ico/png）。
- [x] **CP-4.6 CI**（2026-07-05）
  GitHub Actions：rustfmt/clippy -D warnings/cargo test/vue-tsc/vitest/bindings 新鲜度/hud 体积断言。
- [x] **CP-4.7 M4 验收**（2026-07-05：空闲内存 144.5 MB ≤150 预算（debug 构建）· 空闲 CPU 0.0% · 错密钥/断网/超时错误分类已由 wiremock 集成测试覆盖；真机慢网人工项留发布前回归）
  性能预算自查（07 §12 可测项）；错误注入测试（断网/错密钥/慢网）。

## M5 · 发布准备（需项目所有者参与的项单独标注）

- [x] **CP-5.1 发布工程**（2026-07-05，签名凭据待项目所有者配置）
  release.yml（tag → 三平台构建 + updater 清单）；tauri-plugin-updater 接入。
- [x] **CP-5.2 文档**（2026-07-05，签名凭据待项目所有者配置）
  README 完善、CONTRIBUTING、release-checklist.md、issue 模板。
- [x] **CP-5.3 安全走查**（2026-07-06：日志 redact 复核 ✓ · npm audit ✓ · cargo audit ✓——修复 quick-xml×2/time 三个 RUSTSEC 漏洞（升级 plist 1.10 / time 0.3.47，MSRV 1.85→1.88）+ 更新被 yank 的 num-bigint；余 19 条为 unmaintained/unsound 级 warning，均来自 tauri/rdev 传递依赖，无 fix 版本，记录在案）
  密钥不落盘/不进日志复核；`cargo audit` / `npm audit`；权限最小化复核。
- [ ] **CP-5.4 签名与分发**（⚠ 需 Apple Developer / SignPath 凭据，项目所有者操作）

## 明确不在本路线内（设计书 v1.1+）

本地模型（F-12/ADR-20/22）、个人词典（F-10）、按应用 Profile（F-11）、流式转写、Windows/Linux 平台后端实测（代码留有 trait 扩展位，本机为 macOS）。

---

## v1.0 功能对照清单（2026-07-05 全量审计，逐条对照设计书 02/05 章）

### 已实现 ✅

| 设计书条目 | 实现位置 |
|---|---|
| F-1 听写主干：PTT/toggle 双模（350ms 自动区分）、Esc/✕ 取消、录音→VAD 裁剪→STT→整理→注入 | `hotkey/` `audio/` `orchestrator/` |
| F-1 无时长硬上限：超 provider 上限在静音处切片分段转写拼接 | `providers/stt/mod.rs::transcribe_auto_chunk` |
| F-1 失败不丢话：指数退避重试 ×2、音频/转写稿保留、HUD 重试/复制原文 | `session.rs` `http.rs` |
| F-2 翻译：组合键乐观启动/无缝升级、双向判向提示词、流式收齐一次注入、失败注入原文 | `session.rs` `pipeline.rs` |
| F-2 HUD 方向徽标 + 点击快切最近语言、托盘翻译目标 ▸ | `Hud.vue` `tray.rs` |
| F-3a 选中文本读取（AX 主路径 + Cmd+C 剪贴板降级/哨兵防误读）、原地替换、ANSWER: 判定 | `selection/` `assistant.rs` |
| F-3b 单轮问答：面板流式 Markdown（sanitize）、上下文芯片、动作行、失焦隐藏/📌 | `Assistant.vue` |
| F-4 四槽位 BYOK：openai_compat STT + chat_completions/responses LLM、keyring 密钥、预设填充器、测试连接、多档案切换（设置 + 托盘） | `providers/` `ProvidersPage.vue` |
| F-5 全修饰键三角方案、组合键让路、AltGr 不误判、HotkeyRecorder 改键 + 冲突警告 | `hotkey/` `HotkeyRecorder.vue` |
| F-6 托盘：状态行/完整菜单/模型 ▸ 档案切换/暂停/复制上次结果；单实例二次启动唤起设置 | `tray.rs` |
| F-7 历史：SQLite WAL、保留期清理、主页统计卡组 + 双栏对照 + 搜索/删除/清空 | `history/` `Home.vue` |
| F-8 Onboarding 五步（语言切换/权限轮询/云端直填/练习框/完成） | `Onboarding.vue` |
| F-9 整理层：内置提示词（03 §3.4 逐字）、失败/超时/未配置全降级直通、原样模式、HUD「未整理」 | `pipeline.rs` `prompt.rs` |
| HUD 全状态胶囊（NSPanel 不抢焦点、底部居中 48px、波形 Canvas、成功回弹、失败驻留） | `Hud.vue` `windows.rs` |
| 设置窗口 9 页、提示词模板编辑器（占位符校验/恢复默认）、诊断页权限自检 | `settings/pages/` |
| 提示音三枚（合成木琴系 <150ms）、错误码 i18n 契约（zh-CN/en）、深浅双主题、Dock 图标点击开主页 | `chime.rs` `i18n/` |
| CI（fmt/clippy/test/vue-tsc/vitest/HUD 体积断言）+ release 三平台矩阵工作流 | `.github/workflows/` |

### 未实现 / 部分实现 ❌（补齐 checkpoint）

**P0 缺口（v1.0 应补齐）**

- [x] **CP-6.1 volcengine STT adapter**（2026-07-06：adapter + registry 双凭据构建（app_key/access_token，resource_id 可经 options 覆盖）· ProfileEditor 按 kind 渲染 APP ID/Access Token 双字段 + STT 预设「火山引擎 · 豆包（极速版）」· wiremock 集成测试 5 例（请求形状/鉴权不重试/55 系重试/缺 header 回退 HTTP 状态/自定义 resource_id））
  火山/豆包极速版 flash：双凭据 header、base64 JSON body、`X-Api-Status-Code` 判定（03 §2.2）；ProviderCard 编辑表单按 kind 渲染 AppKey/AccessToken 双字段；wiremock 集成测试。当前 `registry.rs` 为报错占位。
- [x] **CP-6.2 开机自启**（2026-07-06：plugin 初始化（macOS LaunchAgent）· 启动时对齐设置 + settings watch 跟随开关 · onboarding 完成步骤加默认勾选的自启选项）
  `tauri-plugin-autostart` 已在依赖但未初始化；接通用页开关与 onboarding 完成步骤的默认开启。
- [x] **CP-6.3 自动更新**（2026-07-06：plugin 接入 + createUpdaterArtifacts + endpoints（typex.ink 主源 / GitHub 备源，pubkey 待 CP-5.4 签名密钥）· 启动 10s 后自动检查（release 构建、可关）→ `update://available` 事件 · 托盘「检查更新」发事件并打开设置 · 关于页确认卡片（下载并安装→重启，ADR-11 安装需确认）· check_update/install_update commands）
  `tauri-plugin-updater` 接入：默认自动检查、手动确认安装；托盘「检查更新」当前是空操作；设置-关于「检查更新」按钮接真实逻辑。
- [x] **CP-6.4 麦克风权限检测 + 设备选择**（2026-07-06：AVCaptureDevice 麦克风授权检测 + IOHIDCheckAccess 输入监听检测（check_all 三项齐）· Info.plist NSMicrophoneUsageDescription · cpal 输入设备枚举 command · 听写页麦克风下拉接真实设备并写入 dictation.microphone（录音启动已按该值选设备））
  权限检测目前仅辅助功能（`permissions.rs::check_all`）；补麦克风（AVCaptureDevice / tauri-plugin-macos-permissions）与输入监听（IOHIDCheckAccess）实时状态；听写页麦克风下拉接 cpal 设备枚举（当前只有「系统默认」占位）。
- [x] **CP-6.5 HUD 细节补齐**（2026-07-06：快照增 busy_hint 字段——重按忽略时 HUD 轻晃 0.35s + 「正在处理上一条…」微文案（reduced-motion 降级）· 录音超 10 分钟温和提示 · 听写模式点徽标一键切原样（toggle_verbatim command，02 F-9 双入口））
  录音超 10 分钟温和提示；重按忽略时的轻晃 +「正在处理上一条…」微文案（EmitBusyHint 已有 Effect，HUD 未渲染）；HUD 处一键切原样模式入口（02 F-9：HUD 与设置均可切换）。
- [x] **CP-6.6 i18n 全量化**（2026-07-06：i18n 资源 53→251 key（settings/home/onboarding/assistant/components 全节）· 4 窗口挂 vue-i18n + useLocale 跟随「界面语言」设置（system=系统语言）· onboarding 第 1 步语言切换直写设置即时生效全 UI · HUD 保持静态 JSON 极简纪律 · 新增 zh/en key 对齐契约测试；提示词模板/语言列表/厂商名按业务数据保留）
  当前仅 HUD 与错误码走 i18n 资源；设置/引导/主页/助手界面文案为硬编码中文——全部迁入 vue-i18n 并跟随「界面语言」设置（含 onboarding 第 1 步语言切换即时生效于全 UI）。

**P1 缺口（可延 v1.x）**

- [x] **CP-6.7 逐字输入注入后端**（2026-07-06：TypeDirectInjector（enigo text()）· 默认链 paste→type_direct 双后端 · inject_with 按 dictation.inject_method 首选排序（Auto=默认序，失败自动落到后备）· 排序/回退/Auto 三条具名单测）
  设置里已有「逐字输入」选项但无对应后端（仅 paste）；enigo text() 实现 + 后备链接入。
- [x] **CP-6.8 目标应用识别**（2026-07-06：`platform/focus.rs`（NSWorkspace frontmostApplication 本地化名）· 录音开始时采样注入目标 · 历史 app_name 真实写入；其他平台返回 None 留扩展位）
  `platform/focus.rs`（NSWorkspace frontmostApplication）；历史记录 app_name 当前恒为空；HUD NoFocus 检测同源。
- [x] **CP-6.9 托盘图标状态动画**（2026-07-06：运行时渲染 22×22 RGBA 帧——空闲静态五柱（template）/ 录音实时电平 8fps / 处理左→右呼吸 40%↔100% / 暂停 40% 透明+斜杠 / 错误红点角标（非 template 真彩帧）；空闲期零重绘；render 纯函数 5 条单测）
  录音中电平动画（8fps）+ 红点角标、处理中呼吸、暂停 40% 透明 + 斜杠、错误红点；当前仅静态图标 + 状态行文字。
- [x] **CP-6.10 提示词评测脚本**（2026-07-06：`scripts/eval-prompts.ts`（`pnpm eval:prompts [denoise|translate|rewrite] [--limit N]`，TYPEX_EVAL_* 环境变量供密钥）——解析三套 fixtures markdown 表（65+44+25 例）→ 真实 API → 「含/不含/长度/无中文/ANSWER: 前缀」要点断言 → 通过率报告；提示词模板与 Rust 逐字同步；真实 API 全量跑归入发布前回归）
  `scripts/eval-prompts.ts`：读 `docs/fixtures/denoise-cases.md` → 真实 API → 要点断言 + 通过率报告；语料已备，脚本未写。
- [x] **CP-6.11 日志 redact 层 + 诊断包导出**（2026-07-06：RedactWriter 写入层——所有日志行落盘/上屏前自动打码 Bearer token 与 sk- 形态密钥（4 条单测）· export_diagnostics command：环境自检 + 凭据剔除的 settings + 最近 3 个日志文件（再过一遍 redact）打 zip 到下载目录 · 诊断页「导出诊断包」按钮）
  当前靠编码纪律（日志语句不含内容/凭据）；补 tracing Layer 级自动脱敏与「导出诊断包」按钮。
- [x] **CP-6.12 契约快照测试**（2026-07-06：`tests/contract_snapshots.rs`——wiremock 捕获四个 adapter（openai_compat/volcengine/chat_completions/responses）构造的完整 HTTP 请求（method/path/headers/body），boundary/request-id/host/content-length 归一化后 insta JSON 快照，任何请求形状漂移在 diff 显形）
  insta 依赖已加未使用；对四个 adapter 的完整 HTTP 请求形状做快照，防 AI 顺手重构漂移。
- [x] **CP-6.13 回答弹窗杂项**（2026-07-06：选区读取报错（≠无选区）经 selection_read_failed 标志传到 AssistantStartedEvent.degraded——弹窗芯片行提示「读取选区失败，已按普通提问处理」· history.typing_wpm 设置（默认 45，滑杆 15–120）——主页「节省时间」统计按其折算）
  读不到选区时降级为普通提问的提示；打字基准 45 字/分 可调设置。
- [x] **CP-6.14 release-checklist.md**（2026-07-06：`docs/release-checklist.md`——安装首启/F-1/F-2/F-3 验收逐条/注入矩阵/错误注入/主题界面/更新迁移/资源隐私/发布工程十节可勾选清单；当前范围仅 macOS，Windows/Linux 项随 CP-7.x 追加）
  按 02 章验收标准展开为可勾选人工回归清单（发布门槛）。

**平台缺口（设计书要求、本机无法验证，随 Windows/Linux 环境补）**

- [ ] **CP-7.1 Windows 后端**：SendInput 注入、WH_KEYBOARD_LL Alt 短按吞 keyup、UIA 读选区、Credential Manager 实测
- [ ] **CP-7.2 Linux X11 后端**：XTEST 注入、primary selection、AppImage/deb/rpm 出包
- [ ] **CP-7.3 Wayland 分级支持**（07 §8 Tier 1-3）：ashpd Portal 快捷键、wtype/ydotool、gtk-layer-shell HUD、诊断页 compositor 探测

**v1.1 里程碑见下节**；更远期（优先级待定，06 章 v1.x 清单）：个人词典（F-10）、按应用 Profile（F-11）、流式转写、DashScope/Deepgram/ElevenLabs adapter、官方 STT 套餐预设（ADR-16，依赖服务端上线）。

---

## M8 · v1.1 本地模型（F-12 / ADR-20 / ADR-22）

> 目标：消灭「必须先配 API 才能用」的上手门槛。全部工作在 feature flag `local-models` 下隔离，
> 不进 v1.0 依赖树（07 §1）；模型不随安装包分发，应用内按需下载（03 §8）。
> 顺序按收益排列：先本地 STT（收益最大、质量无折扣），本地 LLM 随后（ADR-20 节奏）。

- [x] **CP-8.1 模型库清单 + 硬件分档探测**（2026-07-06：`local/manifest.rs` 6 条目（id/用途/引擎/文件+字节+SHA-256 占位/许可证/HF+ModelScope 双源/min_ram/requires_gpu）· `local/hardware.rs` sysinfo RAM/核数 + Metal 探测 → 轻量/标准/性能推荐（分档边界单测）· 诊断报告加 hardware 字段；全部锁 `local-models` feature，默认构建零影响）
  内置 JSON 清单（id/用途/文件列表/字节数/SHA-256/许可证/双源 URL/最低硬件要求；v1.1 起始 6 个条目：SenseVoice-int8 230MB、Qwen3-ASR-0.6B/1.7B Q8_0、Qwen3.5-0.8B/2B/4B Q4）；`sysinfo` 探测 RAM/CPU 核数 + Metal/CUDA/Vulkan 可用性 → 轻量/标准/性能三档推荐（ADR-22 分档表）；探测结果进诊断页。
- [x] **CP-8.2 模型下载管理器**（2026-07-06：`local/download.rs`——HTTP Range 断点续传（.part 续传）+ SHA-256 校验 + 失败换源重试 + 进度回调；存储 `{data_dir}/models/{model_id}/`；list_downloaded/delete_model；wiremock 集成测试 9 例（Range 形状/续传/校验失败换源/进度单调）；Tauri commands 待 v1.1 开启 feature 时接入 runner）
  HuggingFace + ModelScope 双源（首包延迟自动择优，可固定）；HTTP Range 断点续传、SHA-256 校验、失败换源重试；进度经 Tauri event 推送；存储 `{app_data_dir}/models/{model_id}/`；下载是本地 Provider 唯一网络行为（零上报承诺不变）。
- [x] **CP-8.3 本地 STT Provider · SenseVoice 轻量档**（2026-07-06：`local/stt_sense_voice.rs`——sherpa-rs 静态链接，同一 SttProvider trait，惰性加载 + unload()；capabilities 不限音频时长；模型未下载 → InvalidRequest 不 panic；3 条单测；真实模型推理属人工回归）
  sherpa-onnx 官方 crate 静态链接 + SenseVoice-Small int8；实现同一 `SttProvider` trait（`kind: local`，无 base_url/凭据）；`capabilities()` 报告不限音频时长；错误分类只剩 InvalidRequest/模型未下载。弱机器上唯一保证实时的选项（ADR-22）。
- [x] **CP-8.4 本地 STT Provider · Qwen3-ASR 标准/性能档**（2026-07-06：`local/stt_qwen_asr.rs`——llama-cpp-2 mtmd 音频路径（MtmdBitmap::from_audio_data → eval_chunks → 贪心解码）；主 GGUF + mmproj 双文件（manifest 已补官方 ggml-org Q8_0/mmproj 条目）；max_bytes=10MB 强制 VAD 切片规避长音频 bug；1.7B 条目 requires_gpu；3 条单测）
  llama.cpp（llama-cpp-2 绑定，qwen3vl 音频架构官方 GGUF）跑 0.6B/1.7B；1.7B 仅 GPU 加速可用时提供（纯 CPU 低于实时）；llama.cpp 音频长音频 bug 用现有 VAD 切片路径规避（短分段转写本来就是 F-1 路径）。
- [x] **CP-8.5 本地 LLM Provider（整理/翻译槽）**（2026-07-06：`local/llm_llama.rs`——同一 LlmProvider trait，专属线程推理 → mpsc → BoxStream 流式 delta 与云端一致；n_ctx 4K；GGUF 内置 chat 模板 + ChatML 兜底；LoadPolicy 常驻/用完即卸 + unload()；LlamaBackend OnceLock 单例 + 日志静默；3 条单测；槽位限制在 CP-8.6 registry 层执行）
  llama.cpp + Qwen3.5 0.8B/2B/4B Q4_K_M instruct；实现同一 `LlmProvider` trait（流式 delta 与云端一致）；运行时策略可选：常驻内存 / 录音时预热（冷加载 1–3s）；上下文 4K；**槽位限制：只允许绑定整理与翻译槽，问答槽默认不提供**（性能档设备允许设置中手动指向 4B，ADR-22）。
- [x] **CP-8.6 零配置兜底 + 槽位混搭**（2026-07-06：ProviderKind 增 Local · registry `profile_for_slot` 未配置时合成本地档案（清单序取已下载最轻模型；Assistant 槽显式无兜底，具名单测×2）· build_stt/build_llm Local 分支按清单条目选引擎（sherpa/llama mtmd/llama LLM）· runner 注入 app_data_dir；本地与云端天然槽位级混搭（各槽独立 active_profile））
  STT/整理/翻译三槽在未配置任何档案时默认指向 local 档案（模型已下载前提）；本地与云端槽位级自由混搭；问答槽无兜底，未配置时助手面板显示配置引导。
- [x] **CP-8.7 设置 UI：本地 Provider 卡片 + 已下载模型管理**（2026-07-06：IPC commands ×5（list_local_models/get_hardware_tier/download_local_model/cancel_local_download/delete_local_model，双 feature 组合均编译，默认构建返回空/NotConfigured）+ `local://download-progress` 事件 · presets 加「本地 · 离线」（问答槽过滤）· ProfileEditor local 编辑态（模型下拉按槽位 purpose 过滤 + 加载策略存 options.load_policy + 行内下载）· ProvidersPage 本地副标题 + 「管理…」→ ModelManager 子页（已下载/可下载/硬件检测/占用合计；下载源切换 UI 因 schema 无字段暂缓）；05 §5.1 / mockup 2.7/2.9）
  预设列表加「本地 · 离线」（问答槽预设列表不出现）；卡片副标题显示引擎与模型状态（已下载·体积 / 未下载[下载] / 下载中进度条）；编辑态字段 = 模型下拉（来自模型库）+ 加载策略，无端点/密钥；「测试」= 本地跑内置样音/ping；「管理…」子页 = 已下载列表（体积/被哪些槽使用/删除警告）+ 可下载列表（行内硬件要求与本机检测结果）+ 占用合计 + 下载源切换。
- [x] **CP-8.8 Onboarding 第 3 步「使用本地模型」一键路径**（2026-07-06：推荐卡显示检测档位 + 档位内两模型合计体积 + 「更改档位 ▾」三档下拉 · 点击串行下载（进度条 = --text-1 实底；可取消/重试；不阻塞余下步骤）· 完成后建 local-<model_id> 档案并 activate STT/整理/翻译三槽（问答槽不指向，ADR-20）；跳过路径不受影响；05 §6 / mockup 步骤 3/3b）
  推荐卡显示检测出的档位与体积（可「更改档位 ▾」）；点击即下载（进度条、可后台继续、完成前可先走完余下步骤）；完成后 STT/整理/翻译三槽指向本地。
- [~] **CP-8.9 托盘/主菜单联动 + 回归**（2026-07-06：托盘「模型 ▸」按 profiles 列、local 档案入库自然出现（已验证无 kind 过滤）✓ · README 离线能力说明 ✓ · ⚠ 性能预算复核（整理短句 ≤500ms、常驻内存空闲预算）与 denoise 语料本地评测需真实模型下载 + 实机推理，留 v1.1 发布回归；manifest SHA-256 占位同期补齐）
  托盘「模型 ▸」子菜单含本地档案；性能预算复核（整理短句 ≤500ms、常驻内存模式的空闲内存新预算）；denoise 语料集对本地小模型跑一轮评测报告；README/文档更新离线能力说明。

**v1.1 依赖增量**：`llama-cpp-2`、`sherpa-rs`（或官方 sherpa-onnx crate）、`sysinfo`——全部锁在 `local-models` feature 后；安装包体积增量预算 +30–60 MB（推理引擎静态链接，07 §1）。
