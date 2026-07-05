# 06 · 路线图

> Typex 产品设计书 · 第六章
> 里程碑按「每个阶段结束都有可自用的东西」组织；估时假设 1–2 名全职开发 + AI 辅助编码。

---

## M0 · 骨架（1–2 周）

目标：三平台能跑起来的空壳，管线端到端打通（写死配置）。

- Tauri 2 工程初始化（Vue 3 + TS + Tailwind + tauri-specta；CI 三平台矩阵构建出包）。
- 托盘 + 单实例 + 设置存储（tauri-plugin-store 骨架）。
- rdev 全局按键监听（macOS/Windows/X11）：右 Ctrl 按住/松开事件。
- cpal 录音 → rubato 重采样 16k mono → hound WAV。
- 写死的 openai_compat STT 调用 + 剪贴板粘贴注入。
- **验收**：三平台上按住右 Ctrl 说话，松开后文字出现在光标处（无 UI、无配置）。

## M1 · 听写可用（2–3 周）

目标：F-1 + F-9 完整体验，可以日常自用。

- 会话状态机（orchestrator）+ HUD 全状态（录音/波形/处理/成功/失败/重试）。
- F-9 文本整理：LlmProvider（chat_completions + responses）+ 整理提示词 + 失败降级 + 原样模式开关 + 测试样例集。
- 设置窗口：模型服务页（STT/整理槽、预设模板、keyring 密钥、连接测试）、通用页、听写页、快捷键页（HotkeyRecorder）。
- Onboarding 5 步（含 macOS 权限检测引导）。
- VAD 静音裁剪；长录音自动切片；提示音三枚。
- **验收**：[02 功能规格 F-1 验收标准](02-features.md) 全数通过（Linux 先只承诺 X11）。

## M2 · 翻译（1–2 周）

- F-2：翻译键、翻译流水线、目标语言与双向语言对、HUD 翻译徽标与语言快切、翻译设置页。
- 翻译失败「注入原文」降级。
- **验收**：F-2 验收标准全数通过。

## M3 · 文本处理与问答（2–3 周）

- 选中文本读取（AX / UIA / primary selection + 剪贴板降级链）。
- 助手面板（浮窗、上下文芯片、语音+键盘输入、流式 Markdown、动作行）。
- F-3a 原地替换（含「改写 vs 回答」判定）与 F-3b 单轮问答；助手设置页。
- **验收**：F-3 验收标准全数通过。

## M4 · Linux 深化 + 打磨（2–3 周）

- Wayland：ashpd Portal 快捷键、wtype/ydotool 注入后端、GNOME HUD 降级、诊断页环境自检。
- webkit2gtk 兼容（DMABUF 探测）、AppImage/deb/rpm 三格式。
- 历史记录（F-7）、深浅主题全面走查、动效与提示音打磨、i18n（中/英）。
- 性能预算达标验证（[07 §12](07-code-architecture.md) 全表）；错误注入测试（断网/慢网/错密钥）。

## M5 · v1.0 发布（1–2 周）

- 签名与公证（macOS Developer ID + notarization；Windows 走 SignPath——[ADR-11](09-decisions.md)）。
- 自动更新（tauri-plugin-updater + GitHub Releases）。
- **仓库转公开**（[ADR-15](09-decisions.md)：此前保持私有）：README（含 GIF 演示）、用户文档、CONTRIBUTING、issue 模板 + 发布帖；官网单页（typex.ink）。
- 分发：GitHub Releases + Homebrew Cask（[ADR-17](09-decisions.md)）。
- 安全走查：密钥不落盘/不进日志复核、权限最小化复核、依赖 `cargo audit`。
- 发布 checklist + 三平台人工回归。

**v1.0 总计约 9–15 周。**

## v1.x 及以后（优先级待定）

1. 本地离线转写（whisper.cpp / sherpa-onnx SenseVoice，F-12）——隐私与离线杀手锏，**已确认排入 v1.1（[ADR-13](09-decisions.md)）**。
2. **官方 STT 套餐服务上线**（[ADR-16](09-decisions.md)/D-16）：服务端建设独立于本仓库；客户端仅需新增一个官方预设（openai_compat 通道，零代码特权）。
3. 个人词典/热词（F-10）。
4. 按应用输出 Profile（F-11）。
5. 流式转写（实时字幕式上屏；OpenAI Realtime / 火山二进制 WS / Deepgram adapter）。
6. STT/LLM 更多 adapter（DashScope、Deepgram、ElevenLabs）。
7. 助手面板多轮对话 or 外部 Agent 接入（重新评估，见 ADR-1 复审条件）。
8. 更多分发渠道（winget / AUR / Flathub，视需求）；移动端探索。

## 风险清单

| 风险 | 等级 | 缓解 |
|---|---|---|
| Wayland 碎片化导致 Linux 体验参差 | 高 | 分级承诺（Tier 1/2/3）+ 诊断页自检 + 文档明示；X11 先行 |
| 右侧修饰键被用户重映射（Karabiner/PowerToys）或键盘缺键（小配列） | 低 | 全修饰键三角方案已最小化系统冲突（ADR-7）；onboarding 第 4 步当场改键 + HotkeyRecorder 冲突警告 |
| macOS 权限流（辅助功能/输入监听/麦克风弹窗 bug）卡首次体验 | 高 | onboarding 实时检测 + tauri-plugin-macos-permissions；签名版专项测试 tauri#9928 |
| 整理层「过度改写」损伤信任 | 中 | 提示词强约束「宁欠勿过」+ 样例集回归 + 原样模式一键可达 |
| 无签名证书时 Gatekeeper/SmartScreen 拦截劝退用户 | 中 | 已定购买 Apple Developer + SignPath（ADR-11）；文档提供绕过指引 |
| 依赖的社区 crate（rdev 等）维护停滞 | 中 | 关键路径薄封装（trait 后端可替换）；必要时 fork |
| 名称商标风险（Tipp-Ex） | 低 | 见 D-13；发布前复查 |
| BYOK 配置门槛劝退小白用户 | 中 | 预设模板 + 一个密钥可跑 + 文档「5 分钟上手」路径 |
