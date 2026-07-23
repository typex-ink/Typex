# 09 · 发布人工回归清单

> Typex 产品设计书 · 第九章
> 按 [02 功能规格](02-features.md) 各功能验收标准展开的可勾选清单，**发布前人工回归参考**。
> 当前必过平台是 macOS 12+ 与 Windows x64（Windows 10 22H2+ / Windows 11）；Ubuntu X11 / KDE Wayland 项随对应平台后端适配推进补齐。
> 本文件作为长期维护的检查清单使用；发版时人工参考执行，不要求逐版本复制归档。

## 1. 安装与首启

- [ ] 全新机器或清理平台配置目录后安装 → 首启弹出 onboarding；macOS 与 Windows 分别使用平台标准目录
- [ ] Onboarding 5 步全流程走通：语言切换即时生效 → 平台权限（macOS：麦克风/辅助功能/输入监听；Windows：麦克风）状态正确；macOS 首次点击麦克风「去授权」立即出现系统授权框，允许后无需先按快捷键即显示已授权，拒绝后再次点击才跳系统设置 → 模型直填（STT + LLM）保存成功 → 听写/助手/翻译三个快捷键均可独立录制且练习框能收到听写文字 → 完成后主页自动打开并获得焦点，引导窗口随后关闭
- [ ] 模拟主页打开失败：完成按钮解除提交状态、引导页保留且显示失败提示，可再次点击重试
- [ ] Onboarding「跳过」路径：不配模型直接走完，主界面正常、听写时报「未配置」而非崩溃
- [ ] 完成页「登录时自动启动」勾选生效（系统设置-登录项可见 Typex）
- [ ] 模型管理中低于 RAM/GPU 建议但有远程源的模型仍可直接开始下载；无远程源条目保持不可下载
- [ ] 二次启动（已在运行时再次打开）唤起设置窗口而非双实例

## 2. F-1 听写验收标准

- [ ] 受控输入 harness 与平台代表应用各注入一次中文与英文（含标点、emoji、换行）；Windows 代表矩阵为记事本 / Edge / VS Code / Windows Terminal
- [ ] 5 秒中文短句：松开到上屏 P50 < 2 s（整理开）；原样模式 < 1.5 s（Groq/豆包级 Provider）
- [ ] 口语样句（含语气词/重复/改口）→ 上屏为整理后干净文本
- [ ] 断网说一句 → HUD 明确报错 + 重试按钮；恢复网络点重试 → 成功上屏（音频不丢）
- [ ] 15 分钟连续口述：不中断、不丢内容（长录音自动切片）
- [ ] 中文输入法激活状态下注入无乱码
- [ ] 长按（≥350ms）= 按住说话；短按 = toggle 开始/结束；Esc 与 HUD ✕ 取消不出字
- [ ] 活动会话按 Esc 后目标应用收不到该次 down/repeat/up；空闲、关闭「Esc 取消」或注入已提交时目标应用收到完整 Esc down/up，退出动画期间的新 Esc 同样透传
- [ ] 录音超 10 分钟 HUD 出现温和提示（不中断）
- [ ] 处理中重按触发键 → HUD 轻晃 + 「正在处理上一条…」，会话不受影响

## 3. F-2 翻译验收标准

- [ ] macOS 右 ⌘ + 右 ⌥、Windows 右 Ctrl + 右 Alt 组合触发翻译；录音中追加第二键无缝升级为翻译
- [ ] 说中文出英文、说英文出中文（双向开）各 5 句抽测，无一句注入原文未翻译；提示词改动时对照 `docs/fixtures/translate-cases.md` 扩大抽测
- [ ] 关闭「双向翻译」后说英文 → 仍按 中→EN 方向输出（不再自动判向）
- [ ] 松开到译文上屏 P50 < 2.5 s
- [ ] 口述列表结构（「第一…第二…」）译文保留结构
- [ ] STT 成功但翻译失败（临时改错翻译槽密钥）→ HUD 提供「注入原文」，点击后原文上屏
- [ ] HUD 徽标显示方向（如 `中 → EN`），点击快切目标语言；托盘「翻译目标 ▸」切换即时生效

## 4. F-3 助手验收标准

- [ ] 受控 AX/UIA harness 与平台代表应用选中文本 → 口述「翻译成英文」→ 原地替换，全程无弹窗，HUD ✓ 结束
- [ ] 选中报错日志问「这是什么原因」→ 弹窗展示回答，选区未被替换
- [ ] 无选区按住助手键提问 → 屏幕上 1/3 居中弹窗流式回答
- [ ] 弹窗出现后点击其他应用 → 自动关闭；Esc / ✕ 同样关闭
- [ ] 读不到选区的应用（部分终端）→ 自动降级普通提问 + 弹窗提示「读取选区失败」
- [ ] 弹窗首 token < 1.5 s（主流云端模型）
- [ ] 连续两次提问互不影响（第二问回答不含第一问内容）

## 5. 注入矩阵

| 应用 | 中文 | 英文 | 备注 |
|---|---|---|---|
| 受控文本输入 harness | [ ] | [ ] | Unicode、换行、错目标、只读；真实 UIPI 另见 §11 人工项 |
| 原生文本控件（macOS TextEdit / Windows 记事本） | [ ] | [ ] | |
| VS Code | [ ] | [ ] | Electron / Monaco |
| 浏览器地址栏（Safari / Edge） | [ ] | [ ] | Chromium 与原生浏览器覆盖 |
| 终端（Terminal.app / Windows Terminal） | [ ] | [ ] | 「逐字输入」后备路径也测一次 |
| 无输入焦点（桌面） | [ ] | — | 结果进剪贴板 + HUD 提示 |

微信 / Slack / Word / Chrome / Firefox 为非阻塞兼容抽测，不要求每台回归机预装或登录。若发现崩溃、数据/剪贴板损坏、隐私泄露、错窗口注入，或问题可在受控 harness 复现，则升级为发布阻塞项。

## 6. 错误注入

- [ ] 断网：听写/翻译/助手各报对应错误分类文案，重试可恢复
- [ ] 错密钥：报「密钥无效」且**不**自动重试；测试连接按钮同样分类
- [ ] 慢网（系统级限速或代理延迟）：超时分类正确、HUD 显示耗时
- [ ] 无麦克风权限：录音启动报权限错误 + 引导入口；诊断页 ✗ 显示
- [ ] macOS 撤销麦克风/辅助功能/输入监听权限：诊断页能看出 ✗；点击引导进入系统设置重新开启后，返回 Typex 时无需重载页面即恢复 ✓；Windows 低级钩子安装失败时诊断页显示明确降级
- [ ] macOS TextEdit 中验证：仅成功取消 Typex 会话的 Esc 被消费，空闲 Esc 与所有其他键盘/鼠标事件正常到达；event tap 超时恢复后快捷键继续可用
- [ ] Windows 向管理员权限目标注入：不自动提权，结果进入剪贴板并显示 `InjectionBlocked` 提示

## 7. 界面与主题

- [ ] 深/浅主题全窗口走查（HUD/设置 9 页/主页/onboarding/回答弹窗），无硬编码色残留
- [ ] `系统` 主题跟随 macOS / Windows 外观切换即时生效
- [ ] 界面语言切换（中/英/系统）全 UI 即时生效
- [ ] 系统「减弱动态效果」开启后：HUD 无 spring/晃动动画，功能不受影响
- [ ] 托盘图标状态：空闲静态 / 录音电平动画 / 处理呼吸 / 暂停 40%+斜杠 / 错误红点

## 8. 更新与迁移

- [ ] 从上一版本覆盖安装：settings.json 保留、密钥仍可用、历史库完好
- [ ] 「检查更新」：stable 通道只检查正式 release，nightly 通道检查最新 nightly build；最新版报「已是最新」；旧版本装机检查出新版本 → 确认卡片 → 下载安装 → 自动重启为新版
- [ ] 在 macOS/Windows 当前用户可写的标准安装目录更新时不出现管理员认证；把旧版安装到受保护目录后更新，仅在写权限探测失败后出现一次平台原生认证，成功后以普通用户身份启动新版
- [ ] 取消或拒绝更新安装器的管理员认证：更新终止且旧版文件完整、仍可手动启动；不得反复弹窗或留下新旧文件混合状态
- [ ] 启动 10s 后自动检查不阻塞主流程（关闭「自动检查」后不发请求）

## 9. 资源与隐私

- [ ] 空闲内存 ≤ 150 MB、空闲 CPU ≈ 0%（06 §12；Activity Monitor / Task Manager 观察 10 分钟）
- [ ] 录音中 CPU 占用合理（< 30% 单核）
- [ ] 日志文件抽查：无转写内容、无 Bearer/sk- 形态密钥（redact 层生效）
- [ ] 导出诊断包：settings 中 credentials 为空、日志已脱敏
- [ ] 抓包（Proxyman/Charles）：除用户配置的 API 端点与更新检查外无任何外发请求

## 10. 发布工程

- [ ] `cargo test` / `pnpm test` / `clippy -D warnings` / `pnpm build` 全绿（CI 同步绿）
- [ ] 如本次改动提示词：对照 `docs/fixtures/` 语料完成本地评测并记录结果
- [ ] 版本号符合 [10 版本策略](10-versioning.md)：`package.json` / `tauri.conf.json` / `Cargo.toml` / 关于页 / tag 一致，正式版不带 `-dev`
- [ ] release workflow 的非发布构建/静态校验覆盖 macOS universal 与 Windows NSIS x64；平台 updater fragment 聚合后 `latest.json` 的平台键唯一且 stable/nightly 隔离
- [ ] 签名/公证启用后：从 GitHub Release 下载的 dmg 在全新 macOS 上可直接打开（Gatekeeper 通过）

## 11. Windows 专项

交互桌面 harness 不属于默认测试集；在已解锁桌面显式串行运行：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test windows_platform_harness -- --ignored --test-threads=1
```

- [ ] Windows 10 22H2+ 与 Windows 11 x64 构建基线通过；Rust default / `--no-default-features` 均完成 check、clippy、test
- [ ] debug/release 与安装器内 `typex.exe` 均为 Windows GUI subsystem；开发命令从已有终端启动时日志仍可见，直接启动与登录启动均不出现 CMD 窗口
- [ ] 开机自启启用后 HKCU Run 值为带引号的当前 EXE 完整路径；从旧 debug 或旧安装路径启动时自动修复，关闭开关会删除残留条目，一致状态下不会重复改写
- [ ] 右 Ctrl、右 Alt、独立翻译 chord、默认两键乱序升级、349/351 ms、toggle、Esc、漏 release 恢复均符合规格
- [ ] 中文输入法与 AltGr 原始事件 fixture 不触发误录音、HUD 或提示音；Typex 自身 SendInput 不反向触发热键
- [ ] WASAPI `f32/i16/u16` 转换与实际 USB 麦克风录音通过；设备拔出/切换可恢复且不崩溃
- [ ] HUD fixture 通过 `WS_EX_NOACTIVATE` 与显示前后台 HWND 不变契约；100% DPI 本机位置正确，mixed-DPI/负坐标由坐标测试覆盖，额外实机条件可用时抽测
- [ ] UIA worker 纯逻辑测试覆盖空/非空选区、不支持、COM/Internal 错误、超时熔断、队列故障和多 range/bounds；显式 harness 覆盖真实 UIA、无 TextPattern 的 Ctrl+C 后备与 bounds
- [ ] 向管理员权限目标进程实际注入时被 UIPI 拒绝，不自动提权，结果进入剪贴板并显示 `InjectionBlocked`；这是人工边界，可选使用独立 elevated fixture，纯完整性比较单测不能替代该项
- [ ] 记事本、Edge、VS Code、Windows Terminal 的 UIA/注入行为完成代表性人工抽测
- [ ] 清理历史安装登记后的 NSIS 全新 GUI/静默安装默认进入 `%LOCALAPPDATA%\Programs\Typex` 且无 UAC；覆盖升级与显式 `/D=` 自定义目录保持原路径；目标不可写时只提升安装器并在完成后降权启动 Typex；卸载、重装通过，WebView2 缺失路径由配置/受控 smoke 验证，卸载默认保留设置、历史和模型
- [ ] 作为手动安装和 Tauri 2 updater 共用资产的 NSIS `.exe` 解包后，EXE 同目录包含 sherpa/ONNX 四 DLL、`msvcp140.dll`、`vcruntime140.dll`、`vcruntime140_1.dll`、`vcomp140.dll`、`vulkan-1.dll` 与 runtime manifest，第三方许可位于 `licenses/`；相对路径与文件哈希均和 staging 一致
- [ ] 在未预装 VC++ Redistributable、没有系统级 Vulkan loader/可用 Vulkan ICD 的干净 Windows 基线首启成功，本地模型诊断显示 CPU fallback 而不是进程装载失败
- [ ] 每个平台的 Tauri updater artifact 都使用配置的 `TAURI_UPDATER_PUBKEY` 对 `.sig` 完成实际验签，manifest schema 校验通过；Windows manifest 直接引用 NSIS `.exe` 而非 legacy `.nsis.zip`；未接入 SignPath 时 artifact 明确标记 unsigned，不把 SHA256 或 updater 签名误称为 Authenticode
