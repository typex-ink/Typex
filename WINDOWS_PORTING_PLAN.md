# Typex Windows 平台适配执行计划

> 文档性质：非规范性的执行计划与验收索引，不替代 `docs/` 中的长期规格。
> 规划基线：2026-07-10，代码基线 `8602719`。
> 当前状态：Windows 主体适配、本机验收与 PR CI 已于 2026-07-10 完成；PR #1 保持打开且未发布。
> 执行方式：一次性完成全部工作；阶段仅表示内部依赖顺序，不进行分批交付。执行检查点和验证摘要直接维护在本文件，不同步 GitHub 进度。

## 1. 快速回顾

### 1.1 目标

在不破坏现有 macOS 行为、隐私边界和分层规则的前提下，让 Typex 在 Windows x64 上达到以下状态：

1. 可在 Windows CI 中稳定编译、测试和打包。
2. 右 Ctrl / 右 Alt / 两键组合可可靠触发听写、助手和翻译，且不干扰 AltGr、输入法和普通组合键。
3. WASAPI 录音、HUD、云端 Provider、文本注入、UI Automation 选区读取形成完整闭环。
4. 托盘、单实例、开机自启、设置、主页、onboarding、诊断和自动更新符合 Windows 习惯。
5. 本地模型具备 CPU 可用基线和 Vulkan GPU 加速，并在 GPU 不可用或失败时自动回退 CPU。
6. NSIS 安装器、Tauri updater 签名、受控测试环境和人工回归全部通过；Authenticode 发布者签名作为公开分发前的后置加固，不阻塞 Windows 功能适配完成。

关键路径：

```text
规格/支持边界
  -> Windows 可编译基线
  -> 平台能力与诊断
  -> 低级热键 + WASAPI
  -> 焦点/HUD + SendInput
  -> UIA 选区与回答窗
  -> 本地模型运行库
  -> NSIS/updater/Authenticode 插入点
  -> Windows 发布回归
```

下面的垂直切片只是实施早期的内部检查点，用于尽快验证最危险的系统链路：

```text
Windows x64 + 云端 Provider + 右 Ctrl + WASAPI + SendInput Ctrl+V + 不抢焦点 HUD
```

它不会作为交付结果或独立版本发布。一次性执行会继续完成 UIA 选区、助手/翻译、本地模型、托盘/诊断、CI、NSIS 和 updater，最终交付口径是第 5 节全部阶段完成。

### 1.2 当前判断

当前仓库已有可复用的状态机、Provider、历史、设置、前端窗口和部分跨平台 crate，但没有 Windows 专用平台模块、选择性键盘拦截、UIA、UIPI 诊断、Windows CI 或发布链。当前 Windows 主机已经具备 Rust、Node、pnpm、MSVC、Windows SDK 和本地模型构建工具；尚未完成的是用这些工具建立仓库的 Windows 编译与运行基线。

这意味着适配工作应从“建立真实构建基线”开始，不能直接从修 UI 或制作安装器开始。

## 2. 范围与边界

### 2.1 首发范围

- 架构：`x86_64-pc-windows-msvc`，不使用 GNU ABI。
- 系统：Windows 10 22H2+ 与 Windows 11，不承诺 Windows 10 1809/LTSC 2019。
- WebView：WebView2 Evergreen；NSIS 内嵌 Evergreen Bootstrapper，缺失运行时时联网安装，不捆绑完整离线运行时。
- 分发产物：只构建 NSIS x64 用户安装器；Tauri updater 签名必须完成。SignPath Authenticode 作为后置发布加固，不阻塞 Windows 功能适配。
- 功能：F-1/F-2/F-3/F-4/F-5/F-6/F-8/F-9 必须达到规格；主页、历史、词典和设置不得退化。
- 本地模型：轻量/标准档 CPU 路径纳入完整适配；Windows GPU 加速采用 Vulkan，不维护 CUDA 构建。
- 语言：简体中文与英文。
- 卸载：默认保留设置、历史和已下载模型；用户数据通过应用内入口主动清空，卸载器不静默删除。

### 2.2 明确不在首发范围

- Windows ARM64、32 位 Windows、Windows 7/8。
- Microsoft Store/MSIX、winget、企业 MSI/GPO 部署。MSI 可在 NSIS 稳定后单独评估。
- UAC secure desktop、锁屏和登录界面的注入。
- 从普通权限 Typex 向高完整性管理员窗口强行注入。该场景应检测并降级为复制，不默认建议用户长期以管理员身份运行 Typex。
- 对远程桌面、游戏反作弊、沙箱应用和安全输入框承诺 100% 注入成功；这些场景只做兼容性抽测和明确降级。
- Linux 适配工作。

### 2.3 必须保持的产品行为

- 失败不丢话：音频或已有转写稿必须可重试/复制。
- HUD 不得夺走目标应用焦点，并应显示在前台窗口所在显示器。
- 默认右 Ctrl / 右 Alt / 两键组合，保留 350 ms 长短按、组合升级、组合键让路和漏 release 自恢复。
- 非美式布局、中文输入法、AltGr 下不得乱码或破坏正常输入。
- Windows 选区主路径为 UI Automation，失败才走 `Ctrl+C` 剪贴板降级。
- 无焦点或 UIPI 阻止注入时，结果进入剪贴板并给出明确提示。
- 不记录音频内容、转写内容、选区文本、密钥或凭据。
- 空闲 CPU、内存、启动速度和 HUD 延迟继续受 `docs/06-code-architecture.md` 的预算约束。

## 3. 事实基线

| 领域 | 已有基础 | Windows 缺口 | 当前证据 |
|---|---|---|---|
| 构建依赖 | Tauri bundle 已启用，已有 `icon.ico` | `llama-cpp-2` 默认固定 Metal；Apple 依赖未全部 target-gate；当前无法验证 MSVC 构建 | `src-tauri/Cargo.toml:18,54,61,92` |
| 热键 | 纯 `HotkeyDetector`、Windows 默认键、AltGr/让路/漏 release 单测可复用 | 后端是 listen-only，无法吞右 Alt keyup；AltGr 打字会短暂启动会话 | `hotkey/rdev_backend.rs:1,46`，`hotkey/mod.rs:295` |
| 音频 | cpal、重采样、VAD、波形管线均可复用 | 输入流固定假设 `f32`；未覆盖 WASAPI 的 `i16/u16`、设备拔出和隐私拒绝 | `audio/recorder.rs:72-92` |
| 焦点与注入 | paste -> type_direct 后备链、失败复制语义已有 | 非 macOS 使用通用 enigo；无原生 SendInput 返回值、NoFocus、UIPI、完整性级别判断 | `inject/mod.rs:24-30`，`inject/paste.rs:40-57` |
| 选区 | macOS AX 与跨平台 `Ctrl+C` 哨兵降级已有 | 非 macOS 直接走剪贴板；无 UIA TextPattern、无选区 bounds | `selection/mod.rs:27-35,299-350` |
| 平台层 | `platform` 分层已经存在 | 没有 `platform/windows.rs`；权限为空；前台应用名恒为 `None` | `platform/mod.rs:5-6`，`permissions.rs:86-89`，`focus.rs:15-18` |
| HUD/窗口 | Tauri 配置已有透明、置顶、`focus:false`、skipTaskbar | 无 Win32 no-activate 保证；定位依据 HUD 自身 monitor，混合 DPI/负坐标风险高 | `tauri.conf.json:14-30`，`app/windows.rs:211-225` |
| 前端/托盘 | Windows 左键打开主页逻辑已有；ICO 尺寸齐全 | onboarding 写死 Command；AltGr 显示为 Option；托盘动画固定白色并依赖 macOS template；设置 accelerator 写死 Cmd | `Onboarding.vue:358-367`，`i18n/*:59-65`，`tray_icon.rs:69-80`，`tray.rs:95-97` |
| 诊断/系统操作 | 已有诊断页和导出诊断包 | Windows 权限为空，注入后端文案写死 CGEvent；打开日志目录调用 `open` | `app/commands.rs:264-288` |
| 本地模型 | 下载、清单、SenseVoice/Qwen/llama.cpp 业务逻辑已有 | GPU 探测恒 false；Metal feature 固定；Windows runtime DLL/静态链接闭包未验证 | `local/hardware.rs:93-103`，`Cargo.toml:61` |
| CI/发布 | publish job 已按平台 artifact 聚合的方向设计 | CI、nightly、release 仅 macOS；无 NSIS、SignPath、Windows updater 资产 | `.github/workflows/ci.yml`，`release.yml:64-67`，`nightly.yml:19-22` |

## 4. 实施前决策

以下决定已经冻结；阶段 0 先把产品/架构决定同步到相应 `docs/`，必要时增加 ADR，再修改代码。

| 决策 | 最终选择 | 实施约束 |
|---|---|---|
| 最低 Windows 版本 | Windows 10 22H2+ / Windows 11 | 先同步 01/06/09 与 README，不再维护 1809/LTSC 回归 |
| 架构与安装器 | `x86_64-pc-windows-msvc` + NSIS | 不产出 ARM64、32 位、MSI、MSIX 或 winget 包 |
| Win32 绑定 | target-specific `windows` crate | 所有 `unsafe` 保持在 `platform/` 或对应平台 backend 的最小边界 |
| 热键实现 | 新增 `hotkey/windows_backend.rs` | 直接管理 `WH_KEYBOARD_LL` 和消息循环，复用纯判定器，不扩大 vendored rdev patch |
| 注入实现 | 原生 SendInput paste/direct backend | 通用 enigo 只作非 Windows 路径或最后后备 |
| WebView2 | NSIS 内嵌 Evergreen Bootstrapper | 运行时缺失时联网安装，不捆绑完整离线 runtime |
| 管理员目标应用 | UIPI 命中后复制降级 | 不自动提权，不要求 Typex 长期以管理员身份运行 |
| 本地 GPU | CPU 保底 + Vulkan | 覆盖 Intel/AMD/NVIDIA；不维护 CUDA 构建，失败自动回退 CPU |
| 平台上下文 | 单一 `usePlatform()` 来源 | 优先补齐仓库已声明的 `tauri-plugin-os`，禁止散落 UA 判断 |
| 卸载数据 | 默认保留设置、历史和模型 | 用户通过应用内清理入口主动删除，不在卸载器中静默清除 |
| updater 密钥 | macOS/Windows 复用现有 Repository Secrets | 复用 `TAURI_SIGNING_PRIVATE_KEY`、密码和公钥，不创建 Windows 专用密钥 |
| updater 清单 | 平台 fragment -> 唯一 `latest.json` | stable/nightly 分离并做 schema、重复 key 和 URL 校验 |
| Windows 发布者签名 | SignPath Authenticode 后置 | 与 Tauri updater 签名独立；预留稳定签名插入点，正式大范围发布前再接入 |

## 5. 分阶段执行

### 阶段 0：规格冻结与 Windows 构建基线

对应工作包：`WIN-01`、`WIN-02`。

任务：

1. 把第 4 节已冻结决定同步到 01/05/06/07/08/09/10 与 README 中的相关位置。
2. 在干净 Windows 开发机安装并记录：Visual Studio 或 Build Tools（Desktop C++、Windows SDK；当前主机为 Visual Studio 2026）、Rust MSVC、Node 24、pnpm 10、CMake/Ninja/LLVM（若本地模型需要）、WebView2。
3. 分别运行默认 feature 与 `--no-default-features` 的 check/clippy/test，先形成失败清单再逐项修复；本计划只记录结论，不粘贴大段构建输出。
4. 把 Apple-only 依赖和 feature 收回 macOS target；为 Windows Win32 API 增加最小 feature 集。
5. 在 `windows-latest` 增加最小 PR 门禁，先证明 Rust/前端可编译，不在此阶段承诺 GUI 系统能力。
6. 在 Win10 22H2/Win11 实测经典桌面应用的麦克风隐私行为；若与“首次使用弹系统授权窗”的现有规格不符，先更新 05/06/09，再实现 UI。
7. 为 NSIS、WebView2、Vulkan、UIPI 降级、卸载数据保留和共享 updater 密钥建立可验证的配置基线。

出口门槛：

- Windows MSVC 下 default 和 `--no-default-features` 均能 `cargo check`。
- `cargo test --no-default-features`、`pnpm build`、`pnpm test` 通过。
- 最小 Tauri 应用可以启动、退出并产生托盘图标。
- 所有支持边界都已在本计划冻结并有明确验收，不再存在“后面再决定”的发布阻塞项。

### 阶段 1：Windows 平台骨架、窗口壳与诊断

对应工作包：`WIN-03`、`WIN-06`、`WIN-09`、`WIN-10`。

任务：

1. 新增 `platform/windows.rs`，集中前台 HWND/PID/应用名、monitor/work area、进程完整性、系统主题、设置 URI 和 shell 打开目录等平台胶水。
2. 区分内部原生句柄与可共享的 `FocusInfo`；IPC、日志和历史只暴露应用名等非敏感字段，不暴露 HWND 或窗口标题全文。
3. 将诊断从静态字符串改为真实能力：麦克风、低级钩子、SendInput、UIA、WebView2、注入后端、完整性级别、本地运行库/GPU。
4. 为 Windows 实现日志目录打开、麦克风隐私设置跳转和可恢复错误提示。
5. 建立前端单一平台上下文；onboarding、主页和设置从当前 settings 读取快捷键，不再写死 `MetaRight`。
6. 平台化 Right Alt/AltGr、Ctrl、Win、Menu 键的显示和冲突提示；补中英文测试。
7. 修正 Windows 原生标题、首帧主题、标题栏布局和拖拽区，避免 macOS 红绿灯留白出现在 Windows。
8. 托盘按系统深浅色渲染可见图标；Windows 录音态增加红点；设置 accelerator 使用 Ctrl 语义。

出口门槛：

- 标准用户启动后无多余窗口，托盘左右键、设置、主页、退出、单实例和开机自启均工作。
- 深浅主题下托盘可见，设置/主页/onboarding 首帧无明显白闪。
- 诊断内容与实际平台一致，不出现 CGEvent、Command、Option、Metal 等错误平台文案。
- Win10/Win11 的麦克风设置入口可达；无权限时有可操作提示。

### 阶段 2：低级热键与 WASAPI 录音

对应工作包：`WIN-04`、`WIN-05`。

任务：

1. 实现 `WH_KEYBOARD_LL` 专属线程和消息循环，把 Win32 down/up 事件转换后送入现有 `HotkeyDetector`。
2. 识别 `VK_RCONTROL`、`VK_RMENU`、扩展键、`LLKHF_INJECTED`，避免 Typex 自己的 SendInput 反向触发热键。
3. 仅在已确认的 Typex 右 Alt 手势中吞对应 keyup；普通 AltGr/菜单操作完全放行。
4. 修复 AltGr+字母当前会短暂显示 HUD/播放提示音的问题，要求“让路”在用户可感知副作用前完成。
5. 将 hook 安装失败、线程退出和漏 release 自恢复状态暴露给诊断；失败时提示改键或重启，而非静默失效。
6. 按 cpal `SampleFormat` 分派 `f32/i16/u16`（及实际设备报告的其他格式），统一归一化为 mono f32。
7. 将实时 callback 改为有界、低分配传输；明确溢出策略并记录丢弃计数，不在 callback 中日志或阻塞。
8. 分类无默认设备、隐私拒绝、设备拔出、设备切换和 stream error；固定设备优先使用稳定 ID，名称只作展示。

自动化重点：

- Windows 原始按键序列表驱动：右 Ctrl、右 Alt、组合乱序、349/351 ms、toggle、AltGr 假 Ctrl、普通键让路、注入事件、漏 release。
- “语义事件”和“是否吞原始事件”分别断言。
- `i16/u16/f32 -> f32` 边界、声道混合、采样率转换、环形缓冲溢出测试。

出口门槛：

- US、微软拼音、德语 QWERTZ/AltGr 下默认三组快捷键通过。
- AltGr+E、Ctrl+C、PowerToys 重映射不出现菜单激活、提示音、HUD 或残留录音。
- 内置、USB、蓝牙麦克风均有真实波形；无权限/拔出设备时错误可恢复。
- 连续录音 15 分钟不崩溃、不无限增长内存、不丢尾部样本。

### 阶段 3：焦点、HUD 与 Windows 文本注入

对应工作包：`WIN-06`、`WIN-07`。

任务：

1. 在录音开始时采样目标应用和前台窗口；注入前重新验证目标是否仍有效，避免注入到 Typex 自己的窗口。
2. Windows HUD 使用 `WS_EX_NOACTIVATE` / tool-window / topmost 语义，并以 no-activate 方式显示；显示前后前台 HWND 必须不变。
3. 依据前台 HWND 所在 monitor 的 work area 定位 HUD，统一物理/逻辑坐标，覆盖负坐标、多任务栏和每显示器 DPI。
4. 实现原生 SendInput Ctrl+V；检查发送计数，并将失败分类为 NoFocus、UIPI/权限或内部错误。
5. 实现 `KEYEVENTF_UNICODE` 直接输入，正确处理 UTF-16 surrogate pair、emoji、换行和 keyup。
6. 设计剪贴板事务：打开剪贴板重试、保存规格承诺支持的文本/图片、写入、粘贴、延迟、仅在用户未再次修改剪贴板时恢复。
7. 高完整性目标、无焦点或全部后端失败时保留最终文本到剪贴板，HUD 给出 Ctrl+V 提示。

出口门槛，也即内部可用性检查点 A（不对外发布）：

- 云端听写 F-1 在受控 Win32/UIA harness、记事本、Edge、VS Code、Windows Terminal 中可注入中英文、标点、emoji、换行。
- 中文 IME、非美式布局下无乱码。
- HUD 不抢焦点，100/125/150/200% DPI 与负坐标副屏定位正确。
- 普通 Typex -> 管理员目标应用稳定降级复制，绝不静默丢结果。
- 断网、无焦点、错密钥和注入失败均满足“不丢话”。

### 阶段 4：UI Automation 选区与助手/翻译闭环

对应工作包：`WIN-08`、`WIN-09`。

任务：

1. 建立专用 COM 工作线程并显式初始化 apartment；UIA 访问设置 300 ms 超时，不能阻塞 Tauri 主线程。
2. 使用 TextPattern/GetSelection 读取选中文本；明确区分“支持但空选区”“不支持”“权限/UIPI 失败”。
3. 同次读取返回文本和 bounds；统一 UIA 物理坐标、Tauri 逻辑坐标和 monitor 原点。
4. UIA 不可用时走 Windows Ctrl+C 降级，复用 SendInput 和剪贴板事务，避免复制整行/旧剪贴板误判。
5. 回答窗在选区下方/上方定位，无选区时位于前台屏上 1/3；获得焦点后不再读取目标选区。
6. 回答窗失焦、Esc、关闭按钮行为在 Windows 验证；改写型流程全程不弹窗。
7. 完成翻译方向、助手回答/改写、读取失败降级和旧 request/session 回调隔离回归。

出口门槛，也即内部可用性检查点 B（不对外发布）：

- F-2 中文/英文双向翻译和失败注入原文路径通过。
- 受控 UIA harness、Edge、VS Code、记事本可选中改写；提问时回答窗贴近选区且不替换原文。
- Windows Terminal 等 UIA 不可用场景能走 Ctrl+C 或降级普通提问。
- 多屏混合 DPI 下选区 bounds 和回答窗位置正确。
- 连续两次助手请求无上下文或事件串线。

### 阶段 5：本地模型与 Windows 运行库

对应工作包：`WIN-11`。

任务：

1. 拆分 macOS Metal、Windows CPU/Vulkan Cargo feature；保证 `--no-default-features` 与默认构建都长期可用，不引入 CUDA 构建矩阵。
2. 先跑通 SenseVoice/Whisper CPU 与 llama.cpp CPU，再启用并验证 Vulkan GPU 后端。
3. GPU 探测必须反映“运行时实际可用”，不能只根据显卡存在判断；诊断和模型推荐显示真实 backend。
4. 确认 sherpa/ONNX/llama.cpp 是静态还是动态闭包；若有 DLL，明确收集、资源路径、搜索路径和签名顺序。
5. 在 clean VM 用 `dumpbin /dependents` 或等价检查验证无开发机隐式 DLL 依赖。
6. 覆盖非 ASCII 用户名、长路径、模型下载断点续传、Windows Defender 文件扫描和已加载模型删除失败。
7. 删除已加载模型前先让 registry/provider 释放句柄；文件锁失败必须给出可重试错误。

出口门槛：

- 轻量/标准档从首次下载、校验、加载、转写、整理、重启复用到删除均通过。
- 无 GPU/驱动缺失时可靠回退 CPU，不崩溃、不展示虚假的性能档。
- 安装目录不缺 DLL，日志和诊断不泄露模型输入文本。
- 本地推理带来的包体、内存和延迟偏差有实测记录；若超规格，先更新决策/预算再发布。

### 阶段 6：CI、安装器、签名与 updater

对应工作包：`WIN-12`、`WIN-13`。

任务：

1. PR Windows job 运行 Rust default/no-default check、clippy、test，以及 IPC 生成、前端 build/test。
2. main/nightly 增加 Windows NSIS package smoke；release job产出带版本、架构、渠道的唯一资产名。
3. 配置内嵌 WebView2 Evergreen Bootstrapper、安装范围、Start Menu、卸载、升级/降级和自启动行为；卸载默认保留用户数据。
4. 固定未签名产物、内层二进制和 NSIS 外层安装器的签名插入点；SignPath 可用时按正确顺序做 Authenticode 并校验证书链和时间戳。
5. 使用现有 Repository Secrets 为 Windows 生成 Tauri updater artifact 和 `.sig`，不重复创建平台密钥。明确 Authenticode 是发布者身份签名，Tauri `.sig` 是更新完整性签名，两者不能互相替代。
6. 每个平台上传唯一 manifest fragment；publish job校验完整性后生成唯一 stable/nightly `latest.json`，禁止 merge artifact 时同名覆盖。
7. CI/测试产物允许暂时没有 Authenticode；Tauri signing key、updater pubkey 或 manifest 缺失必须 hard fail。未来正式公开发布启用 SignPath 后，Authenticode 缺失同样 hard fail。
8. 校验 SHA256、安装器签名、嵌套二进制签名、updater schema、下载 URL 和稳定/夜间通道隔离。

出口门槛：

- 本机 Win11 与干净 Windows CI/受控安装环境均可安装、启动、升级、卸载和重新安装；没有现成 Win10/Win11 VM 时不临时启用需要重启的虚拟化功能。
- WebView2 缺失/旧版场景有可理解的安装流程。
- Tauri updater `.sig` 校验通过；SignPath 尚未接入时明确标记安装器为 unsigned，接入后 `Get-AuthenticodeSignature` 与 `signtool verify /pa /all` 必须通过。
- stable 不读取 nightly；nightly 不覆盖 stable release 或正式 tag。
- release 聚合元数据同时包含 macOS 和 Windows，且 `latest.json` 每个平台键唯一。

### 阶段 7：硬化、文档同步与发布就绪审计（不发布）

对应工作包：`WIN-14`、`WIN-15`。

任务：

1. 将 Windows 纳入 `docs/09-release-checklist.md` 的必过平台，按第 8 节完整走查。
2. 运行性能、隐私、休眠/唤醒、锁屏、RDP、默认设备切换、Explorer 重启、崩溃恢复和长时间常驻测试。
3. 抽查日志、诊断包、历史库、临时音频和剪贴板，确保不泄露受保护内容。
4. 用 Defender 和至少一种常见第三方安全软件检查低级键盘钩子、SendInput 和安装器误报。
5. 更新 README/README_ZH 的下载、系统要求、已知限制和签名说明。
6. 同步 02/04/05/06/07/09/10；仅在新增长期决策时更新 08 ADR。
7. 只生成本地/Actions 测试 artifact，不创建版本 tag、nightly tag 或 GitHub Release；发布动作等待项目所有者后续单独授权。

正式完成定义：

- 本计划所有出口门槛都有本地验证证据和结论摘要。
- Windows CI、NSIS 构建与 updater 签名链全绿；本次不要求实际发布新版本。
- F-1/F-2/F-3 以及托盘/onboarding/设置/历史/主页在 Windows 回归通过。
- 无未处置的 P0/P1 缺陷；P2 缺陷必须有明确限制文案和 owner。
- 空闲 CPU 约 0%、内存 <= 150 MB、冷启动 <= 2 s、按键到 HUD <= 100 ms、按键到录音 <= 150 ms。
- 日志和诊断包不含音频、转写、选区、密钥或凭据。
- macOS 全量最低检查仍通过，没有因 Windows 分支造成回归。

## 6. 本地工作包拆分

以下编号只用于一次性执行中的依赖排序、提交边界和回顾，不要求创建 GitHub Issue：

| ID | 交付物 | 主要依赖 | 相对规模 |
|---|---|---|---|
| WIN-01 | 将已冻结的 Windows 版本、UIPI、NSIS/WebView2、Vulkan、卸载和签名边界同步到规格 | 无 | M |
| WIN-02 | MSVC 依赖 target-gating、Windows CI 编译基线 | WIN-01 | L |
| WIN-03 | `platform/windows.rs`、焦点、权限、shell、能力诊断 | WIN-02 | L |
| WIN-04 | `WH_KEYBOARD_LL`、Alt/AltGr、hook 健康状态 | WIN-02, WIN-03 | XL |
| WIN-05 | WASAPI sample format、设备 ID、错误与缓冲 | WIN-02 | L |
| WIN-06 | HUD no-activate、前台 monitor、混合 DPI | WIN-03 | L |
| WIN-07 | SendInput、UIPI、剪贴板事务、NoFocus 降级 | WIN-03, WIN-06 | XL |
| WIN-08 | UIA 选区、bounds、Ctrl+C 后备 | WIN-03, WIN-07 | XL |
| WIN-09 | Windows onboarding、快捷键标签、诊断与错误文案 | WIN-03, WIN-04, WIN-05 | M |
| WIN-10 | Windows 托盘深浅图标、录音红点、菜单习惯 | WIN-02, WIN-03 | M |
| WIN-11 | 本地模型 feature、GPU/backend、runtime DLL | WIN-02 | XL |
| WIN-12 | NSIS、WebView2、Updater、Authenticode 插入点与 clean VM | WIN-02, WIN-11 | XL |
| WIN-13 | stable/nightly updater artifact 与多平台 manifest 聚合 | WIN-12 | L |
| WIN-14 | Windows 功能/安装/升级人工回归 | WIN-04 至 WIN-13 | XL |
| WIN-15 | 性能、隐私、安全软件和发布审计 | WIN-14 | L |

可并行关系：

- WIN-04（热键）、WIN-05（音频）、WIN-10（托盘）可在 WIN-02/03 稳定后并行。
- WIN-11（本地模型）可与 WIN-04 至 WIN-08 并行，但不得打乱云端听写检查点 A 的关键路径。
- WIN-09 可先处理平台上下文和标签，再等待热键/权限能力接入。
- WIN-12/13 不应早于内部检查点 B；安装器不能替代系统能力验收。

## 7. 自动化门禁

### 7.1 Windows PR 最低命令

```powershell
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --no-default-features
cargo clippy --manifest-path src-tauri/Cargo.toml --no-default-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm install --frozen-lockfile
pnpm gen:ipc
pnpm build
pnpm test
```

阶段 6 以后增加：

```powershell
pnpm tauri build --bundles nsis
Get-AuthenticodeSignature <installer-or-exe>
signtool verify /pa /all <installer-or-exe>
dumpbin /dependents <typex.exe>
```

### 7.2 必须新增的自动化用例

- Windows 键盘原始事件解码、事件吞/放行、AltGr、注入事件、stale release。
- cpal sample format 转换、声道混合、缓冲溢出和设备错误映射。
- SendInput INPUT 序列：Ctrl+V、BMP、surrogate pair、emoji、换行、keyup。
- 剪贴板事务：锁竞争、用户中途改剪贴板、恢复失败、不覆盖用户新内容。
- UIPI/完整性比较和 NoFocus 分类的纯逻辑层。
- UIA adapter 的空选区、不支持、超时、错误、多个 range 和 bounds 转换。
- 多 monitor 原点、work area、DPI 的纯坐标转换。
- Windows 快捷键/权限/GPU/i18n 文案，不得出现 macOS 符号。
- Windows 托盘浅色/深色/录音红点帧像素断言。
- 多平台 updater manifest 合并、重复 key、缺签名和错误通道 URL。

真实键盘、真实注入、UIA、权限弹窗和窗口焦点仍按 `docs/07-testing.md` 走人工回归，不在 CI 强上不稳定 GUI 自动化。

## 8. Windows 实机回归矩阵

### 8.1 环境

- Windows 10 22H2 x64。
- Windows 11 当前稳定版 x64。
- 标准用户；管理员目标应用；UAC secure desktop 负向用例。
- WebView2 已有、缺失、旧版。

### 8.2 输入设备与键盘

- 内置阵列麦、USB 麦、蓝牙耳机、无默认设备、录音中拔出、默认设备切换。
- US、微软拼音、德语 QWERTZ/AltGr，至少再抽一套欧洲 AltGr 布局。
- PowerToys 键位重映射、无右修饰键键盘的改键路径。

### 8.3 目标应用

- 受控 Win32/UIA harness、VS Code、Edge、记事本、Windows Terminal。
- 普通权限与管理员权限的 harness/记事本/Terminal 对照。
- 无输入焦点、只读控件、密码框、远程桌面各做负向或降级验证。

WeChat、Slack、Word、Chrome、Firefox 等作为非阻塞兼容抽测，不是 Windows 适配完成的前置条件。只有崩溃、数据/剪贴板损坏、隐私泄露、注入错窗口，或能在受控 harness 复现为系统契约错误时，第三方应用问题才升级为阻塞项。

### 8.4 显示与生命周期

- 单屏与双屏；副屏在主屏左/上方产生负坐标。
- 100%、125%、150%、200% DPI；两个显示器不同缩放。
- 任务栏底/侧/自动隐藏；深浅主题；高对比度；reduced motion。
- 睡眠/唤醒、锁定/解锁、Explorer 重启、RDP 连接/断开、音频设备变化。

### 8.5 发布工程

- 全新安装、覆盖升级、自动更新、降级阻止、卸载、重装。
- 设置/历史/密钥保留策略符合规格，卸载不意外删除用户数据。
- 开机自启、二次启动、托盘恢复、重启后模型可用。
- Defender/SmartScreen、签名证书、时间戳、SHA256、Tauri updater 签名。

## 9. 风险登记

| 风险 | 等级 | 预警信号 | 处置 |
|---|---|---|---|
| 全局键盘钩子干扰 Alt/AltGr 或被安全软件拦截 | 高 | 菜单误激活、打字出现 HUD、Defender 告警 | 专用 backend、最小拦截、事件表测试、签名和安全软件回归 |
| UIPI 阻止向管理员窗口注入 | 高 | SendInput 计数为 0、焦点存在但无上屏 | 预先比较完整性，复制降级，明确文案，不自动提权 |
| WASAPI 默认格式/设备变化导致无声或崩溃 | 高 | 仅部分麦克风可用、拔出后线程退出 | sample format 分派、有界缓冲、设备错误分类和设备矩阵 |
| UIA 在 Electron/终端/Office 表现不一致 | 高 | 空选区误判、COM 超时、bounds 缺失 | 专用 COM 线程、明确结果类型、300 ms 超时、Ctrl+C 后备 |
| 混合 DPI 和负坐标导致 HUD/回答窗错屏 | 高 | 125%+副屏偏移、窗体跨屏 | 明确物理/逻辑坐标边界、纯函数测试、真机矩阵 |
| 剪贴板恢复覆盖用户新内容或丢非文本格式 | 高 | 剪贴板管理器记录异常、图片消失 | sequence/内容校验、格式范围明确、用户修改后不恢复 |
| Metal/ONNX/sherpa/llama.cpp 使 MSVC 构建或 clean VM 启动失败 | 高 | 默认 feature 不编译、缺 DLL | target feature、CPU 基线、依赖闭包检查、clean VM |
| GPU backend 扩大包体或驱动兼容面 | 中高 | 安装包超预算、启动崩溃、性能档反而更慢 | 先 CPU，单一 GPU backend，运行时探测和自动回退 |
| SignPath 尚未接入导致 Unknown Publisher/SmartScreen 提示 | 中 | 安装器需要“更多信息 -> 仍要运行” | 不阻塞适配；保留 SHA256/updater 签名与稳定 Authenticode 插入点，公开发布前再申请 |
| 多平台 `latest.json` 同名覆盖 | 高 | release 只有一个平台 key、updater 404 | fragment + 单点聚合 + schema/重复 key 测试 |
| WebView2 Bootstrapper 在离线/受限网络环境失败 | 中高 | 安装器无法完成、代理环境下载失败 | 明确联网要求、分类错误、代理/重试验证，不捆绑完整 runtime |
| Windows 分支破坏 macOS | 高 | macOS CI、NSPanel、快捷键或本地模型回归 | target isolation，跨平台 trait，macOS 全量门禁保持必过 |

## 10. 文档同步地图

实施 PR 按变更内容先更新规格，再改代码：

| 变更 | 必须同步 |
|---|---|
| Windows 支持版本、功能/降级边界 | `docs/01-product-overview.md`、`docs/02-features.md`、README/README_ZH |
| 权限、onboarding、快捷键、HUD、回答窗、错误文案 | `docs/05-ux-spec.md` |
| 托盘深浅图标或 Windows 视觉规则 | `docs/04-design-system.md` |
| Win32 backend、FocusInfo、依赖、线程、DPI、UIPI、本地 GPU | `docs/06-code-architecture.md` |
| Windows 自动化、CI 和人工回归范围 | `docs/07-testing.md`、`docs/09-release-checklist.md` |
| 安装器、GPU、管理员策略等新的长期取舍 | `docs/08-decisions.md` 新 ADR |
| release 资产、updater 或版本流程 | `docs/10-versioning.md` |
| IPC、ErrorCode、settings schema | 对应 02/05/06 + 生成 bindings + 中英 i18n 测试 |

## 11. 单次执行检查点

执行期间只在发生中断、阶段转换或出现重大阻塞时更新这一小段，不同步 GitHub：

```text
当前状态：Windows 主体适配已完成；本轮补齐 sherpa 官方预编译包、llama.cpp Vulkan 深路径构建、dev/test 原生 CRT 对齐、构建/测试临时目录清理及旧根目录 workaround 移除，本地 commit 已完成并按项目所有者要求暂不 push；PR #1 保持打开，未合并、未发布
当前工作包：feat/windows-support（基线 8602719；实现 bafc63f；follow-up b4e236d、26b7357、ced7b63、02a792f、4af09d7）
最后通过的出口门槛：同一项目 `src-tauri/target` 下两次 clean `pnpm tauri dev` 均启动有效 Windows 窗口，Vulkan 识别 RTX 5080 + Intel Graphics；本地 release EXE、unsigned NSIS、24 文件完整解包、哈希/微软签名/PE 依赖闭包和审计临时目录清理全部通过
最近验证：no-default 284 单测 + 4 快照 + 23 Provider；default 349 单测 + 4 快照 + 10 本地模型 + 23 Provider；交互式 Windows harness 按设计 ignored；前端 111/111；release metadata 5/5；三份 workflow 通过 actionlint；debug Tauri PE 不再依赖 debug CRT；本地 NSIS 16,818,774 bytes，审计前后 `typex-nsis-audit-*` 与 `typex-updater-*` 均为 0；上一轮 GitHub Actions run 29099599320 六项全绿
阻塞：无 P0/P1；SignPath/Authenticode 后置，Windows 10、物理 AltGr、mixed-DPI 多屏、睡醒/RDP、第二/蓝牙麦克风和真实跨完整性等级 UIPI 保留为非阻塞实机矩阵
下一步：等待项目所有者允许后再 push `4af09d7` 并观察 PR #1 新一轮 CI；当前不触发远端动作，保持 PR 未合并且不创建 tag/Release
```

需要回顾或交接时使用以下模板覆盖上面的检查点：

```text
基线 commit：
本阶段范围：
完成工作包 / commits：
自动化证据：
实机环境与结果：
规格/ADR 变更：
新增或关闭风险：
遗留 P0/P1：
下一出口门槛：
```

除检查点外，本计划只在以下情况更新：支持范围改变、阶段依赖改变、验收门槛改变、风险处置策略改变。

## 12. 无人值守执行前置清单

本节记录 2026-07-10 对当前 Windows 主机和 GitHub 仓库的只读预检。它只用于避免长时间执行中途等待用户输入。

### 12.1 已满足

- 当前终端以管理员高完整性运行，`winget` 可用。
- Git 与 GitHub CLI 已安装；GitHub CLI 登录有效，SSH push 验证通过，当前账号对 `typex-ink/Typex` 有 admin/push 权限。
- GitHub Actions 已启用，release workflow 已声明 `contents: write`。
- Repository Secrets 已存在：`TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`、`TAURI_UPDATER_PUBKEY`。Windows 复用它们，不新增平台密钥。
- 主机为 x64 Windows 11 Pro，约 47 GB RAM、89 GB 可用磁盘、NVIDIA RTX 5080 + Intel Graphics；Vulkan runtime 可见。
- Rust 1.97 MSVC、Node 24、Visual Studio 2026 C++ workload、MSVC 14.51、Windows SDK 10.0.26100、CMake、Ninja、LLVM/Clang、Vulkan SDK/glslc 均已安装。
- `signtool`、`dumpbin` 和 WebView2 Evergreen Runtime 已安装；Edge、VS Code、Windows Terminal、记事本可用。
- 当前有一个可用 USB 麦克风，麦克风隐私状态允许桌面应用访问。
- 交流电睡眠和休眠超时均为 0；长任务期间保持接通电源即可。
- 项目所有者已授权：安装/调整开发依赖、创建 commits、push feature branch、创建/更新 PR、触发非发布型 GitHub Actions。明确禁止创建正式/夜间版本 tag、GitHub Release，或执行任何实际发布新版本的操作。

### 12.2 执行过程自行收尾

以下环境差异不需要用户处理：

- 当前 pnpm 为 11.11，执行前固定到项目 CI 使用的 pnpm 10。
- 当前 Codex 进程继承了安装工具前的旧 `PATH`；执行开始时从 Machine/User 环境刷新，或使用已核验的绝对路径，不要求用户重启或重新登录。
- Python/Pillow 尚未安装；只在图标脚本仍需要时安装，或把图标生成流程改为不依赖 Windows 上的 `iconutil`。
- `signtool`/`dumpbin` 不在普通 PATH；通过 VS Developer 环境或绝对路径调用。
- 电池供电仍会在 10 分钟后睡眠，因此执行期间保持接通交流电。

### 12.3 验收边界

完整 Windows 适配以 Windows 系统契约、可靠降级和安全不变量为门槛，不以穷尽所有第三方应用为定义：

- 受控 Win32/UIA harness 必须覆盖标准/只读/无 UIA 控件、焦点、UIPI 分类逻辑、Unicode、剪贴板锁、DPI/负坐标 fixtures、热键原始事件和 WASAPI fake devices；真实跨完整性等级目标另走人工矩阵。
- 当前已有的记事本、Edge、VS Code、Windows Terminal 组成代表性应用矩阵，分别覆盖标准控件、Chromium、Electron/Monaco 和终端风险。
- WeChat、Slack、Word、Chrome 等是非阻塞兼容抽测，不要求预装或登录；单个应用不暴露 UIA/禁用粘贴时，只要 Typex 正确降级就不算适配失败。
- AltGr、多屏 mixed-DPI、第二/蓝牙麦克风、clean VM、真实睡醒/RDP、真实跨完整性等级 UIPI 属于额外实机覆盖。缺少这些条件时用纯逻辑、adapter 和 harness 完整覆盖可控行为，并在最终报告明确未做的物理环境抽测，不中断实现。
- 第三方或物理环境问题只有在导致崩溃、文本/剪贴板损坏、隐私泄露、注入错窗口、无法停止录音，或可在 harness 复现为 Windows 契约错误时才阻塞完成。

### 12.4 不需要提供的密钥

- 不需要 Windows 专用 Tauri updater key。
- 不需要把现有 GitHub Secret 值复制到本机或对话中；workflow 直接引用现有 Secret。
- 实现、单元测试和 Provider 契约测试不需要真实 STT/LLM API key，统一使用 wiremock、fixture 和本地模型。
- 只有要执行真实云端延迟/端到端 smoke 时才需要测试用 STT/LLM 凭据。凭据应放在用户级环境变量或独立的本机 secret store 中，不得粘贴到聊天、计划、仓库或日志。

### 12.5 当前无人值守就绪结论

当前已具备开始一次性实施的条件，执行过程中不再因第三方应用登录、额外硬件或 SignPath 主动询问用户。

SignPath 当前未配置，按已确认策略作为非阻塞后置项：

- 不影响 Rust/Tauri 功能、NSIS、Tauri updater 签名和 CI artifact。
- 未签名安装器会显示 Unknown Publisher，并可能触发 SmartScreen“Windows 已保护你的电脑”；用户通常可通过“更多信息 -> 仍要运行”继续。
- 企业 WDAC/AppLocker/Smart App Control 可能直接拒绝未签名程序，因此正式大范围发布前仍建议申请 SignPath。
- 本次只预留 Authenticode 插入点，不申请、配置或提交 SignPath，不发布任何新版本。
