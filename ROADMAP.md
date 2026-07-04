# Typex 实现路线表

> 依据设计书 `/Users/daodaoneko/Documents/Typex/docs/`（01–09 章 + mockups）生成。
> 每完成一个 checkpoint：将 `[ ]` 改为 `[x]`、附完成日期，并提交一次 git commit。
> 里程碑划分对应设计书 06 章；模块归属对应 07 章；UI 以 `docs/mockups/ui-mono.html` + `tokens.css` 为准。
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
- [ ] **CP-2.4 M2 验收**
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
- [ ] **CP-3.5 M3 验收**
  F-3 验收标准自查（连续提问无泄漏、改写 vs 回答判定样例）。

## M4 · 历史 / 主页 / 完善

- [x] **CP-4.1 历史记录（F-7）**（2026-07-05）
  rusqlite WAL；每条记录含模式/原始转写/结果/目标应用/时长/字数；保留期清理；设置-历史页（开关/期限/清空）；关闭历史仍保「最近 1 条」内存级。
- [x] **CP-4.2 主页窗口**（2026-07-05）
  880×560 侧边栏（品牌行 mini 图标/首页/历史记录/底部 设置+◐ 主题切换）；首页页签（大标题+快捷键提示+统计卡组 2×2+最近 3 条+空状态）；历史页签（搜索/展开双栏对照/复制/删除/清空）——对照 mockup §1。
- [x] **CP-4.3 托盘完整菜单**（2026-07-05）
  状态行（就绪/录音中/处理中/暂停/错误）+ 打开助手/复制上次结果/文本整理开关/翻译目标▸/模型▸（三组档案切换）/暂停/设置/主页/检查更新/退出；托盘图标状态动画（录音红点角标/处理呼吸）。
- [ ] **CP-4.4 诊断页 + 权限**
  环境自检（权限状态/注入后端/compositor 探测，✓/✗+修复引导）；日志（tracing + redact 层：密钥与转写文本不入日志）+ 导出诊断包。
- [ ] **CP-4.5 打磨**
  深浅主题全界面走查（对照 mockup 双主题）；动效规格核对（04 §6）；reduced-motion；应用图标（typex.svg 波形成 T → icns/ico/png）。
- [ ] **CP-4.6 CI**
  GitHub Actions：rustfmt/clippy -D warnings/cargo test/vue-tsc/vitest/bindings 新鲜度/hud 体积断言。
- [ ] **CP-4.7 M4 验收**
  性能预算自查（07 §12 可测项）；错误注入测试（断网/错密钥/慢网）。

## M5 · 发布准备（需项目所有者参与的项单独标注）

- [ ] **CP-5.1 发布工程**
  release.yml（tag → 三平台构建 + updater 清单）；tauri-plugin-updater 接入。
- [ ] **CP-5.2 文档**
  README 完善、CONTRIBUTING、release-checklist.md、issue 模板。
- [ ] **CP-5.3 安全走查**
  密钥不落盘/不进日志复核；`cargo audit` / `npm audit`；权限最小化复核。
- [ ] **CP-5.4 签名与分发**（⚠ 需 Apple Developer / SignPath 凭据，项目所有者操作）

## 明确不在本路线内（设计书 v1.1+）

本地模型（F-12/ADR-20/22）、个人词典（F-10）、按应用 Profile（F-11）、流式转写、Windows/Linux 平台后端实测（代码留有 trait 扩展位，本机为 macOS）。
