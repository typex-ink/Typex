# 06 · 代码架构

> Typex 产品设计书 · 第六章
> 本章是技术架构规格：用什么技术（§1–§2）、代码怎么组织（§3–§5）、各平台系统能力怎么实现（§6–§9）、契约与前端（§10–§11）、性能与质量门槛（§12–§14）、最小垂直切片（§15）。开发中若与本章冲突，先改文档再改代码。
> 技术选型结论基于 2026-07 的生态调研（Handy、Whispering 等同类开源项目源码与 Tauri/crate 现状）。

---

## 1. 技术选型结论

**框架：Tauri 2.x（Rust 后端 + 系统 WebView 前端）。**

| 候选 | 结论 | 理由 |
|---|---|---|
| **Tauri 2** | ✅ 采用 | 安装包 < 20 MB、空闲内存 30–90 MB，满足「输入法级占用」；系统级脏活（键盘监听/注入/录音/AX）在 Rust 生态全有现成 crate；同品类头部开源项目 **Handy**（Tauri 2 + React）与 **Whispering**（Tauri 2 + Svelte）已验证整条路线 |
| Electron | ❌ | 空闲内存 150–300 MB、包体 80–200 MB，与「低占用」定位直接冲突 |
| Flutter Desktop | ❌ | 空闲内存约为 Tauri 3 倍；桌面端系统集成（全局热键、AX、文本注入）插件生态薄弱 |
| 原生三写 | ❌ | 体验上限最高（参考 VoiceInk），但三平台三套代码对开源项目维护成本不可接受 |

**前端：Vue 3 + TypeScript + Tailwind CSS v4**（已定，[ADR-6](08-decisions.md)）。
组合式 API + `<script setup>`；状态管理用 Pinia；构建走 Vite（Tauri 默认脚手架）。参考实现提示：同类项目 Whispering（Svelte）与 Handy（React）的**架构思路**均可借鉴，组件代码需按 Vue 重写。
IPC 使用 **tauri-specta** 自动生成 TS 类型绑定，杜绝前后端接口漂移。波形绘制用 Canvas 2D（60 fps 足够，无需 WebGL）。

**Rust 关键依赖**（2026-07 均为活跃维护状态）：

| 用途 | crate | 备注 |
|---|---|---|
| 全局按键监听（push-to-talk） | `rdev`（macOS grab / X11 listen）+ 原生 Win32 `WH_KEYBOARD_LL` + `ashpd`（Wayland Portal）| macOS 需选择性吞已认领 Esc；Windows 还需吞右 Alt keyup、识别 AltGr 与注入事件 |
| 录音 | `cpal` | 事实标准 |
| 重采样 | `rubato` | 设备原生采样率 → 16 kHz mono |
| VAD | `silero-vad-crs 0.4`（Silero，内嵌权重）+ 能量门限 | 双路径静音裁剪 + 长录音切片边界 |
| WAV 编码 | `hound` | 上传用 |
| 提示音 | `rodio` | 开始/结束/错误音 |
| 文本注入 | `enigo`（模拟 Cmd/Ctrl+V）+ `arboard`（剪贴板存取恢复） | Wayland 另有外部工具后端，见 §8 |
| 读取选中文本 | `get-selected-text`（AX → 剪贴板降级）；必要时直接用 `axuielement` / `uiautomation` | |
| HTTP / SSE | `reqwest` + `eventsource-stream` | 全部 Provider 调用 |
| WebSocket | `tokio-tungstenite` | 火山流式扩展 |
| 密钥存储 | `settings.json` credentials 字段 | 与其他配置项同路径；导出与日志必须脱敏 |
| 本地数据 | `rusqlite`（历史）+ `tauri-plugin-store`（设置） | |
| macOS 权限 | `tauri-plugin-macos-permissions` + `macos-accessibility-client` | 检测 + 引导跳转 |
| 应用基建 | `tauri-plugin-single-instance` / `-autostart` / `-updater` / `-global-shortcut`（备用）；托盘内置 | 全官方插件 |
| Windows 系统 API | target-specific `windows` crate | 最小 feature 集；`unsafe` 集中在 `platform/windows.rs` 与对应平台 backend 边界 |
| Linux HUD | `gtk-layer-shell`（经 `gtk_window()` 句柄） | Handy 验证过的方案 |
| 本地推理（[ADR-20](08-decisions.md)/[ADR-22](08-decisions.md)） | LLM+Qwen3-ASR：`llama-cpp-2`；SenseVoice/Whisper STT：`sherpa-onnx`；硬件探测：`sysinfo` | feature flag `local-models` 集中管理，可 `--no-default-features` 裁剪；`llama-cpp-2` target-gate 为 macOS Metal / Windows Vulkan / 其他 CPU；GPU-loaded 模型仅在运行初始化/decode 错误且尚可安全重放时回退一次 CPU；Windows 的 sherpa/ONNX、Vulkan loader 与 MSVC runtime 采用安装目录内 app-local 部署；硬件仅用于推荐档和性能提示，不阻止存在远程源的模型下载（[03 §8](03-model-providers.md)、[ADR-26](08-decisions.md)） |

## 2. 进程与窗口模型

单进程（Tauri 主进程 = Rust 核心），多 WebView 窗口按需创建/销毁：

| 窗口 | 生命周期 | 特性 |
|---|---|---|
| **HUD** | 常驻（隐藏/显示切换，避免创建延迟） | 无边框、透明、置顶、**不可获得焦点**（macOS 必须用 NSPanel/nonactivating，否则注入目标失焦——经 `tauri-nspanel`）、忽略鼠标事件（除按钮区） |
| **回答弹窗** | 按需显示，关闭即隐藏 | 无边框；原生窗口仅圆角外透明，面板本体使用不透明 `--surface`；禁用原生窗口阴影、不可手动调整大小、原生窗口高度跟随内容、置顶、可获得焦点、优先贴近选区下方、失焦自动关闭（无 pin）、只读展示（无输入能力，见 05 §4） |
| **设置** | 按需创建 | 常规窗口 720×520 |
| **引导** | 首次启动 | 常规窗口 |
| **主页** | 按需 | 常规窗口 880×560：侧边栏导航（首页/历史记录）+ 内容区（统计、最近记录、历史列表） |

录音、转写、注入全部发生在 Rust 侧；WebView 只负责渲染与用户输入。**HUD 隐藏时对应 WebView 应处于空闲（不跑动画循环）**，这是空闲内存/CPU 预算的关键。

## 3. 分层总览

```
┌────────────────────────────────────────────────────────┐
│  前端 (Vue 3, 多窗口 WebView)                            │
│  hud / assistant / settings / onboarding / home         │
└──────────────────────┬─────────────────────────────────┘
                       │  IPC 边界（tauri-specta 生成的类型化契约，见 §10）
┌──────────────────────┴─────────────────────────────────┐
│  app 层（Tauri 壳）：commands / events / 窗口与托盘管理    │
├────────────────────────────────────────────────────────┤
│  orchestrator 层：会话状态机（唯一的业务流程所有者）        │
├────────────────────────────────────────────────────────┤
│  service 层（互不依赖的领域服务）                          │
│  hotkey · audio · inject · selection · providers        │
│  settings · history                                     │
├────────────────────────────────────────────────────────┤
│  platform 层：OS 探测、权限检测、平台专用胶水              │
└────────────────────────────────────────────────────────┘
```

**依赖规则（自上而下单向，禁止反向与横向）**：

1. 前端只认 IPC 契约，不知道 Rust 内部结构。
2. `app` 只做胶水：把 command 转发给 orchestrator/service，把内部事件转成 IPC event。**不写业务逻辑。**
3. `orchestrator` 是唯一可以同时调用多个 service 的地方；service 之间**互相不 import**（audio 不知道 inject 的存在）。
4. service 依赖 `platform` 与公共基础（`error`、`config` 类型）；`platform` 不依赖任何 service。
5. 所有跨层数据用 `types/` 中的共享结构体（serde + specta derive），禁止裸 JSON。

## 4. 仓库总目录树

```
typex/
├── src-tauri/                    # Rust（单 crate，见 §5.6）
│   ├── Cargo.toml
│   ├── tauri.conf.json           # 窗口、updater、bundle 配置
│   ├── capabilities/             # Tauri 2 权限清单（按窗口最小化授权）
│   ├── icons/                    # 构建产物图标（由 assets/icon 脚本生成）
│   └── src/
│       ├── main.rs               # 入口：单实例→托盘→窗口→服务装配（DI 手工装配）
│       ├── app/
│       │   ├── commands.rs       # 全部 #[tauri::command]（薄，仅转发）
│       │   ├── events.rs         # 全部 IPC event 定义与 emit 封装
│       │   ├── tray.rs           # 托盘图标状态机 + 菜单
│       │   └── windows.rs        # 窗口创建/显隐/定位（HUD NSPanel 处理在此）
│       ├── orchestrator/
│       │   ├── mod.rs            # Orchestrator：持有各 service 句柄
│       │   ├── session.rs        # SessionState 状态机 + 转移逻辑（纯逻辑，可单测）
│       │   └── pipeline.rs       # 听写/翻译/助手 三条流水线的步骤编排
│       ├── hotkey/
│       │   ├── mod.rs            # trait HotkeyBackend + HotkeyService（含长短按/组合判定）
│       │   ├── rdev_backend.rs
│       │   └── portal_backend.rs # Wayland (ashpd)
│       ├── audio/
│       │   ├── mod.rs            # AudioService：start/stop/cancel，输出 Recording
│       │   ├── recorder.rs       # cpal 流 + ring buffer
│       │   ├── pipeline.rs       # 重采样(rubato) + 电平计算 + VAD 切片
│       │   └── chime.rs
│       ├── inject/
│       │   ├── mod.rs            # trait Injector + 后备链（InjectorChain）
│       │   ├── paste.rs          # 剪贴板+粘贴+恢复（全平台默认）
│       │   ├── type_direct.rs
│       │   └── wayland.rs        # wtype/ydotool/dotool 外部工具后端
│       ├── selection/
│       │   └── mod.rs            # trait SelectionReader + 平台降级链
│       ├── providers/
│       │   ├── mod.rs            # ProviderRegistry：由配置构造 provider 实例
│       │   ├── error.rs          # ProviderError 分类（§5.4）
│       │   ├── http.rs           # 共享 reqwest 客户端工厂（代理/超时/重试）
│       │   ├── stt/
│       │   │   ├── mod.rs        # trait SttProvider
│       │   │   ├── openai_compat.rs
│       │   │   ├── volcengine.rs
│       │   │   └── local.rs      # Qwen3-ASR(llama.cpp) + SenseVoice(sherpa-onnx)（feature "local-models"）
│       │   ├── llm/
│       │   │   ├── mod.rs        # trait LlmProvider + PromptKit（内置提示词模板）
│       │   │   ├── chat_completions.rs
│       │   │   ├── responses.rs
│       │   │   └── local.rs      # llama.cpp + Qwen3.5（仅整理/翻译槽，feature "local-models"）
│       │   └── models/
│       │       └── mod.rs        # 模型库清单 + 下载管理器 + 硬件分档探测（[03 §8]）
│       ├── settings/
│       │   ├── mod.rs            # SettingsService：读写、校验、变更广播
│       │   ├── schema.rs         # 全部配置结构体 + 默认值 + schema_version
│       │   └── migrate.rs
│       ├── history/
│       │   └── mod.rs            # rusqlite：建表/迁移/CRUD/保留期清理/主页统计聚合（每条记录含时长与字数）
│       ├── platform/
│       │   ├── mod.rs            # 能力探测汇总（DiagnosticsReport）
│       │   ├── permissions.rs    # 各平台权限检测/引导
│       │   ├── macos.rs / windows.rs / linux.rs
│       │   └── focus.rs          # 前台应用/焦点信息（FocusInfo，F-11 预留）
│       ├── types/                # 跨层共享类型（serde + specta derive）
│       │   ├── session.rs        # SessionSnapshot, SessionMode, SessionPhase…
│       │   ├── profile.rs        # ProviderProfile, SlotKind…
│       │   ├── hotkey.rs         # 稳定 KeyId 规范、别名迁移与 chord 归一化
│       │   └── diagnostics.rs
│       ├── error.rs              # TypexError 顶层错误 + 用户可见错误码（§5.4）
│       └── logging.rs            # tracing 初始化 + redact 层
├── src/                          # 前端（Vue 3 + TS + Tailwind，见 §11）
├── assets/                       # 设计源文件：icon/typex.svg、sounds/、测试样音
├── docs/                         # 本设计书 + fixtures/denoise-cases.md
├── scripts/                      # 图标导出、release 辅助脚本
├── .github/workflows/            # ci.yml（lint+test+build 矩阵）、release.yml
├── package.json / vite.config.ts / tailwind.config.ts
└── CONTRIBUTING.md / LICENSE (GPLv3)
```

## 5. Rust 侧设计

### 5.1 服务装配与状态持有

- `main.rs` 手工装配（不引依赖注入框架）：构造 `SettingsService` → 据配置构造 `ProviderRegistry`、`AudioService`、`InjectorChain`、`SelectionReader`、`HotkeyService` → 全部交给 `Orchestrator` → `Orchestrator` 与 `SettingsService` 放入 Tauri `State`（`Arc<...>`）。
- 服务句柄一律 `Arc<Service>`，内部可变性用 `tokio::sync::Mutex/RwLock`（跨 await）或 `parking_lot`（纯同步、短临界区）；**禁止**在持锁状态下 await 网络调用。
- 配置热更新：`SettingsService` 变更后广播 `watch::channel`；`ProviderRegistry` 订阅并按 profile-id 惰性重建 provider 实例，其他服务各自订阅所需字段（如快捷键改绑）。

### 5.2 会话状态机（orchestrator 的核心）

所有三大功能共享同一状态机，仅「处理阶段」策略不同：

```
            ┌──────── cancel (Esc，注入提交前) ─────────────────┐
            ▼                                                    │
Idle ─press─▶ Recording ─release─▶ Transcribing ─▶ Processing ─▶ Injecting ─▶ Done ─▶ Idle
                │                     │  (STT)        │ (F-1:整理/直通         │
                │                     │               │  F-2:翻译              │
                │                     ▼               │  F-3:处理/问答)        ▼
                └──────────────▶   Failed ◀───────────┴──────────── (重试/放弃/复制兜底)
```

```rust
pub enum SessionMode { Dictation, Translation, Assistant }
pub enum SessionPhase {
    Idle,
    Recording   { started_at, mode },
    Transcribing { audio },
    Processing  { transcript },      // F-1 整理 / F-2 翻译 / F-3 LLM 调用
    Injecting   { text },
    Failed      { stage, error, recoverable_payload },  // 保住已有产物：音频或转写稿
}
```

- 状态机本体是**纯函数式转移表**（`fn advance(state, event) -> (state, Vec<Effect>)`），不做 IO；`Effect`（StartRecording / StopRecording / CallStt / Inject / EmitUi / PlayChime…）由 orchestrator 的执行器逐个 dispatch 到 service。松键转移的 Effect 顺序固定为 `EmitUi(Transcribing)` → `StopRecording`；阻塞音频收尾完成后以 `RecordingFinished` 再触发 `CallStt`。**这是全项目单测密度最高的地方**（重按忽略、Esc 取消、组合键让路、失败重试等规则全部在此验证）。
- 任何时刻只有一个活动会话（当前不并行）；每个会话有自增 `session_id`，所有异步回调带 id 校验，杜绝「上一条的转写结果注入到下一条」的竞态。
- 执行器保存当前音频收尾、STT、LLM 与注入任务句柄；取消时终止可终止任务并关闭生成中的助手窗。所有完成事件除 `session_id` 外还必须核对当前 phase，迟到回调不得写历史、注入、弹窗或重新占有音频。
- 每次 phase 变更 → `SessionSnapshot` 经 `app/events.rs` 推送给前端（HUD/面板据此渲染，前端无自己的业务状态机；每个状态携带可显示的进度语义，见 [05 UX 规格 §3](05-ux-spec.md)）。

### 5.3 线程与异步模型

| 执行体 | 形态 | 职责 |
|---|---|---|
| Tauri 主线程 | 事件循环 | 窗口/托盘/菜单（部分平台 API 要求主线程） |
| tokio runtime | Tauri 自带 | orchestrator 执行器、全部 provider HTTP、settings/history IO |
| hotkey 线程 | `std::thread`（macOS 为 rdev grab、X11 为 rdev listen；Windows 为 Win32 消息循环） | 键盘事件 → 判定逻辑 → `mpsc` 发给 orchestrator；macOS/Windows 同步决定 Esc 是否吞键 |
| UIA 线程（Windows） | 专用 COM STA/MTA worker | 初始化 COM、执行 `TextPattern.GetSelection` 与 bounds 查询；请求有超时且不阻塞 Tauri 主线程 |
| cpal 回调线程 | 音频驱动回调 | **只做** ring buffer 写入（实时线程禁止分配/锁/日志） |
| audio worker 线程 | `std::thread` + 结束时 `spawn_blocking` 等待 | 从 ring buffer 取样与电平；候选阶段只积累内存，提升为正式录音后才以 50ms 节流发电平事件；停止流、排空、重采样、VAD 与 WAV 编码不得阻塞 orchestrator 循环 |
| 本地推理 worker | LLM 使用专属 `std::thread`；ASR 使用 `spawn_blocking` | 每个模型缓存用独占 inference lease 串行推理；条目携带 GPU/CPU load mode 与代际；GPU runtime fallback 先释放失败代际，再在缓存锁外以无设备、无 context/mtmd offload 的严格 CPU 参数加载，LLM 在首个可见 delta 前、ASR 在非流式返回前最多从头重试一次 |

跨线程通信统一 `tokio::sync::mpsc`（service → orchestrator）与 `watch`（配置广播）；hotkey/audio 线程持有的发送端是它们与外界的唯一接口。

### 5.4 错误处理

- 底层各 service 用 `thiserror` 定义局部错误；顶层统一收敛为 `TypexError`，其中带 `ErrorCode`（`auth_error` / `network_error` / `timeout` / `rate_limited` / `server_error` / `no_speech` / `no_focus` / `permission_missing` / …）。
- `ErrorCode` 即 [05 §9 错误文案表](05-ux-spec.md) 的键：Rust 只发 code + 参数，**文案在前端 i18n 里**（中英双语一处维护）。
- 重试策略集中在 `providers/http.rs`（对 `rate_limited/server_error/network` 指数退避 ×2），orchestrator 不重复实现。

### 5.5 日志

`tracing` + `tracing-appender` 滚动文件（默认 INFO，诊断页可临时调 DEBUG）。全局 redact 层：`Authorization`、`X-Api-Access-Key`、任何 `credentials` 字段、以及**转写文本本身**（日志只记长度与耗时，不记内容——隐私承诺的一部分）。

### 5.6 关于不拆 workspace 的说明

当前保持单 crate：模块边界靠 §3 依赖规则约束（CI 中可用 `cargo-modules`/review 把关），避免过早抽象。若后续出现「providers 想独立发布」或编译时间失控，再拆 `typex-core` / `typex-providers`——目录结构已按可拆分的形状组织。

## 6. 数据流与隐私边界

```
麦克风 ──▶ Rust 内存（PCM）──▶ 用户配置的 STT 端点（HTTPS）
                                    │
   选中文本（AX/剪贴板）─────────────┼──▶ 用户配置的 LLM 端点（整理/翻译/问答，HTTPS）
                                    ▼
                              注入目标应用
```

- **除用户自己配置的端点外，无任何网络请求**（更新检查除外，可关）。无遥测（见 [ADR-5](08-decisions.md)）。
- 音频不落盘；仅当转写失败等待重试时写入临时目录（内存优先，超大切片才落盘），成功或放弃后立即删除。
- Windows 快捷键确认期间的候选音频严格只驻内存：只有匹配 token 被确认后才能成为会话音频；取消后立即释放，禁止写磁盘、历史、日志、Provider 或前端电平事件。Typex 不常驻占用麦克风，候选只从物理触发键按下开始，因此设备实际回调前的硬件启动时间无法追溯。
- 剪贴板注入前保存原内容、注入后恢复；文档中明示「恢复仅支持文本/图片格式」的已知妥协。

## 7. 关键系统能力实现方案

### 7.1 平台适配矩阵（总览）

当前支持 macOS 与 Windows；Linux 是下一步适配目标。Windows 首发边界为 `x86_64-pc-windows-msvc`、Windows 10 22H2+ 与 Windows 11。

| 能力 | macOS | Windows | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| 全局按住说话 | rdev grab（需辅助功能/输入监听权限） | 原生 `WH_KEYBOARD_LL` 消息循环 | rdev listen（XTEST/XRecord） | Portal GlobalShortcuts（ashpd）；evdev 兜底 |
| 文本注入 | 剪贴板 + CGEvent Cmd+V | 剪贴板 + SendInput Ctrl+V | 剪贴板 + XTEST Ctrl+V | wtype（wlroots 系）/ ydotool·dotool（GNOME/KDE）/ 仅复制降级 |
| 读选中文本 | AX API → 静音 Cmd+C 降级 | UIA TextPattern → Ctrl+C 降级 | primary selection | primary selection（部分可用）→ 仅手动粘贴降级 |
| HUD 置顶浮窗 | NSPanel（不抢焦点） | 原生支持 | 原生支持 | gtk-layer-shell（GNOME 不支持 → 降级为托盘状态） |
| 托盘/自启/单实例 | ✅ | ✅ | ✅ | ✅（托盘图标需落盘 XDG_RUNTIME_DIR 等已知小坑） |

### 7.2 已知平台坑清单（开发时逐条对照）

1. **macOS 权限静默失效**：未授权辅助功能时 rdev event tap 可能静默无事件、不报错——必须用 `macos-accessibility-client` 主动检测并引导；开发时给终端/IDE 授权。CGEventTap 还可能被系统以 `TapDisabledByTimeout/UserInput` 禁用，vendored rdev grab 后端必须收到该事件后立即 `CGEventTapEnable(..., true)`。
2. **macOS 签名后麦克风弹窗 bug**（tauri#9928/#11951）：Info.plist 有 `NSMicrophoneUsageDescription` 也可能不弹授权——需原生侧主动 `AVCaptureDevice.requestAccess`（`tauri-plugin-macos-permissions` 已封装）；entitlement `com.apple.security.device.audio-input`。
3. **cpal 0.16 macOS CoreAudio 枚举 release 崩溃**：CoreAudio 的 `AudioObjectGetPropertyData(Size)` 会写回 `ioDataSize`，上游 0.16.0 macOS 后端若用不可变局部变量承接，在 release/LTO 下可能被优化成 0 长度 buffer 并在 `HALDeviceList::GetData` SIGSEGV；本项目通过 `src-tauri/vendor/cpal` patch 保持该参数可变，升级 cpal 时必须复核。
4. **HUD 抢焦点会毁掉注入**：macOS 必须 NSPanel + nonactivating style；其他平台设置不可聚焦标志。
5. **逐字模拟键入在非美式布局/输入法激活时乱码** → 默认剪贴板粘贴路径；粘贴前 60 ms 级可调延迟（部分慢应用需要）。
6. **X11 组合键 release 事件顺序 bug**（global-hotkey#39）→ 用 rdev 自维护按键状态，不依赖热键 API 的 release。
7. **Windows UIPI**：目标窗口是更高完整性进程时 SendInput 被拦截 → 预先比较完整性级别并检查发送计数；失败后把完整结果留在剪贴板并提示手动粘贴。Typex 不自动提权，也不建议长期以管理员身份运行。
8. **webkit2gtk**：NVIDIA 驱动下白屏/崩溃 → 启动时探测并自动注入 `WEBKIT_DISABLE_DMABUF_RENDERER=1`；仅支持 webkit2gtk-4.1 的发行版（Ubuntu 22.04+）。
9. **剪贴板恢复不保真**（arboard 仅文本/图片）→ 设置页明示；与剪贴板管理器可能互相干扰记录中间内容。
10. **Windows 混合 DPI 与负坐标**：HUD/回答窗定位统一以目标 HWND 的 monitor work area 为源，在物理/逻辑坐标边界显式换算；禁止用 HUD 自身 monitor 推断目标屏幕。
11. **Windows 原生运行库不能依赖开发机环境**：默认 feature 的 EXE 硬链接 sherpa/ONNX、MSVC C++/OpenMP runtime 与 Vulkan loader；NSIS 必须把四个 sherpa/ONNX DLL、`msvcp140.dll`、`vcruntime140.dll`、`vcruntime140_1.dll`、`vcomp140.dll` 和 `vulkan-1.dll` 放在 EXE 同目录。Vulkan loader 存在但没有可用 ICD/GPU 时直接加载 CPU 模型；GPU 模型已加载后的 context/decode 错误仅在无可见输出时从头 CPU 重试一次。不得因缺少 loader 在进程装载阶段退出，也不得在 LLM 已流式输出后重放造成重复文本。
12. **Windows 启动不得闪控制台**：debug/release EXE 均使用 GUI subsystem；debug 仅尝试附着已有父控制台以保留开发日志，失败即静默继续，禁止创建新控制台。登录自启的 HKCU Run 命令固定为带引号的当前 EXE 完整路径，启动对账必须修复旧路径且保持一致状态幂等（[ADR-26](08-decisions.md)）。
13. **Windows 安装路径不主动迁移**：NSIS 保持 `currentUser`；仅当没有历史卸载/产品登记、`$INSTDIR` 仍是 Tauri 原默认值时，installer hook 改用 `%LOCALAPPDATA%\Programs\Typex`。历史安装位置与显式 `/D=` 优先，GUI 与静默安装使用同一判断（[ADR-26](08-decisions.md)）。

### 7.3 快捷键（push-to-talk 细节）

- 不用 `tauri-plugin-global-shortcut` 作为主路径（无法监听单个修饰键；X11 release 有 bug）。macOS 用启用 `unstable_grab` 的 **rdev grab event tap**，X11 暂用 rdev listen-only 独立线程，Windows 用 `WH_KEYBOARD_LL` + 消息循环；三者都把归一化事件送入同一个纯判定器维护 down/up 状态——默认键位为全修饰键三角方案（见 [05 §7.1](05-ux-spec.md)），必须支持单修饰键触发。
- **稳定 `KeyId` 契约**：持久化名称以物理 `KeyboardEvent.code` 为主，至少包括 `Enter`、`Digit0..9`、`ArrowLeft/Right/Up/Down`、`AltLeft/AltRight`、`MetaLeft/MetaRight`、`Menu`、`F1..F19`、`KeyA..Z`、`Semicolon`/`Period`/`Backquote`/`BracketLeft`/`BracketRight`/`Backslash` 与 `Numpad*`。`Menu` 是对浏览器 `ContextMenu` 的稳定例外。前端 code、rdev `Key` 与 Win32 VK/scan 必须在各自 adapter 显式映射，禁止把 crate `Debug` 文本当持久化协议。`Return`→`Enter`、`Num1`→`Digit1`、`LeftArrow`→`ArrowLeft`、`AltGr`→`AltRight`、`Alt`→`AltLeft`、`ContextMenu`→`Menu`、`SemiColon`→`Semicolon`、`Dot`→`Period`、`Kp*`→`Numpad*` 等旧名仅作读取/迁移别名。
- 字母、数字行与标点是**物理位置语义**：浏览器读取 `KeyboardEvent.code`；左右修饰键额外用标准 `KeyboardEvent.location` 校正侧别，以兼容 Windows WebView2 将物理右 Shift 的 `code` 误报为 `ShiftLeft` 的情况。同一物理修饰键的 keydown / keyup 若被误报成不同侧，录制器按 `Shift` / `Control` / `Alt` / `Meta` 家族配对，并在任一事件能确认右侧时保存右侧 `KeyId`；只有录制期间已观察到 `Unidentified` / `Process` keydown 时，后续可识别但无法配对的 keyup 才用于恢复缺失键，其他孤立 keyup 忽略。keyup 的 `ctrlKey` / `shiftKey` / `altKey` / `metaKey` 仍为真时继续监听，防止先释放已识别键后过早提交；等待最终 keyup 超过 2 秒或窗口失焦则静默取消且不修改绑定。`location` 只修正这四类修饰键的左右侧，不参与普通键映射。Windows 普通键优先按低级 hook 的 set-1 scan code 解码（VK 只作 scan 缺失时的后备），rdev 使用其物理 `Key` variant。不得用 `KeyboardEvent.key`、当前布局产生的字符或输入法结果作为绑定 ID。
- 听写、助手与翻译的 `Vec<KeyId>` 各表示一个独立完整 chord；判定器把三组所有按键纳入 global physical-held 集合，只有某 chord 全部 held 才发 `TriggerDown`。独立翻译 chord 可直接启动翻译；若随后完成的 chord 严格包含当前 chord，判定器发 `ModeUpgraded(target_mode)` 让较长 chord 接管并保留音频。没有包含关系且长度不更长的额外完整 chord 不改变本次手势最先启动的模式。partial chord 从未启动时，全释放不得误发 `TriggerUp`；手势已启动后仍等本次 tracked 触发键全部释放才发一次 `TriggerUp`。
- 三组 chord 必须非空；听写与助手不得相同或互为子集，翻译不得与听写或助手完全相同，但允许双方存在严格子集关系。前端在 IPC 前阻止，`SettingsService::update` 再以 `InvalidRequest` 拒绝；启动读取到历史非法值时只恢复 `HotkeySettings::default()` 并写无键值 warning，其他设置不得丢失。
- **组合键让路规则（核心）**：触发键按住期间收到任何**普通键** down 事件 → 判定用户在使用系统组合键（`⌘C`、`Ctrl+C`、`AltGr+E` 等），立即静默让路、不产生任何输出、按键完全放行。Windows adapter 对默认右 Ctrl / 右 Alt 的 `TriggerDown` 语义最多暂存 75 ms，同时立即发出带唯一 token 的内部 `CaptureCandidateStarted`；AudioService 异步开流并只在内存积累样本。窗口内普通键/AltGr 发匹配取消，双触发键或超时确认让 `TriggerDown` 携带同 token 并提升现有流。暂停、配置更新、hook 失败/终止与退出也必须取消未决 token；原始 down 时间戳必须保留用于 350 ms 长短按判定。
- **漏 release 自恢复**：rdev/CGEventTap 偶发漏掉触发键 release 时，`HotkeyDetector` 会残留 held 状态。修饰键正常不会自动连发，因此同一触发键在 250 ms 抖动窗口后再次 down 视为上一轮 release 丢失：旧 held 状态重置，必要时向状态机发 `Yielded` 取消卡住的录音，再发新的 `TriggerDown`，保证下一次按键恢复响应。
- **配置热更新边界**：settings 桥接只广播归一化后确实变化的三组 chord 或 `esc_cancels`，判定器和 Windows adapter 仍须对相同配置幂等；只切换 `esc_cancels` 不得结束或重置当前 chord。任一 chord 改绑时若旧 chord 已触发，backend 必须先发送一次配对 `TriggerUp` 再替换配置；旧物理键随后到达的 release 只用于清理，不得再次结束会话。partial chord 从未触发则直接清空且不产生语义事件；Windows 75 ms 内尚未确认的候选必须发送匹配 token 的取消事件，禁止把未确认手势提交给会话状态机。已确认的 Windows 右 Alt 手势在改绑后保留一次性 release tombstone，只吞对应的下一次物理 RAlt keyup；未确认候选、AltGr、让路和未配置路径不得设置该 tombstone。
- **Esc 会话门闩**：orchestrator 用线程安全门闩发布当前可取消 `session_id`。Recording、Transcribing、Processing、Failed 直接可认领；Injecting 绑定该会话现有 `InjectionLatch`，Esc 取消与首个 OS 输入提交竞争同一原子状态。认领成功只生成一次 `EscPressed { session_id }`，执行器必须校验该 ID 仍是活动会话；Idle、设置关闭、过期 ID 或注入已提交均认领失败。平台 adapter 只对成功认领的物理 Esc 序列吞 down、重复与配对 up，后续新 Esc 和其他键鼠事件完整放行。
- **事件拦截点**：Windows 用现有 `WH_KEYBOARD_LL` 同时处理已确认右 Alt keyup 与已认领 Esc；继续忽略 `LLKHF_INJECTED`。macOS rdev grab callback 除已认领 Esc 外原样返回全部事件，vendored event tap 在系统超时/用户输入禁用后必须立即重新启用。Linux 当前保持 listen-only，待其平台后端适配。
- Windows 事件解码必须区分 `VK_RCONTROL`、`VK_RMENU`、扩展键与 `LLKHF_INJECTED`；Typex 自己的 SendInput 事件不得反向触发会话。右 Ctrl / Ctrl+C、物理右 Alt / 普通键和 AltGr 常见的伪 `Left Ctrl` + `Right Alt` 序列必须经过同一 75 ms 副作用确认边界；只有已确认的右 Alt 助手/翻译手势可以吞对应 keyup。
- Windows hook 的 health watch 是运行期安全信号而不只是诊断查询：从 Healthy 进入 `Failed` 或意外 `Stopped` 时，runner 必须只发送一次会话 `Cancel` 并刷新托盘为「快捷键不可用」，防止漏掉 TriggerUp 后持续占用麦克风。callback panic 或事件通道关闭进入 `Failed` 时，hook state 必须原子禁止后续 raw event、退出消息循环并卸钩，且该 `Failed` 不得在 `WM_QUIT` 收尾时被覆盖为 `Stopped`。启动失败走同一个可订阅 health 状态；应用主动退出使用独立的正常 `Shutdown` 终态，不取消、不报错。
- rdev backend 必须观察暂停 watch 的版本变化而不只读取最终布尔值；任一暂停/恢复 transition 都先清空 detector held 状态，暂停期间到达的 release 不得在恢复后留下 stale gesture。
- 长按/短按判定：press 后 350 ms 内 release = toggle（三种模式一致，含助手）；超过 = push-to-talk（release 即停止）。**乐观启动**：非 Windows 默认修饰键在触发键按下即开始录音；Windows 默认右侧修饰键在 75 ms 内确认，但麦克风候选流从原始 keydown 即开始，确认时原位提升且不重开设备。普通组合键路径取消候选并保持完全静默。
- Wayland：探测 `XDG_SESSION_TYPE`；优先 `ashpd` 走 `org.freedesktop.portal.GlobalShortcuts`（KDE/GNOME≥48/Hyprland 支持，Activated/Deactivated 信号天然支持按住；注意 Portal 快捷键由 compositor 分配，未必能绑到「单独的右⌥」，此时默认键退化为 compositor 允许的组合键）；不可用时提示 evdev 方案（用户加入 `input` 组）或 compositor 绑定 `typex toggle` CLI 命令（经 single-instance 转发）。

### 7.4 录音

- cpal 以设备原生采样率（44.1/48 kHz）开流；callback 内只做 ring buffer 拷贝，重采样（rubato → 16 kHz mono f32）与电平计算在 worker 线程。
- 输入流按 `cpal::SampleFormat` 分派并统一归一化为 mono f32；Windows WASAPI 至少覆盖 `f32`、`i16`、`u16`，不假定默认设备一定输出 f32。callback 使用有界缓冲，溢出只累计丢弃计数，不阻塞、不分配、不记录音频内容。
- HUD 波形数据：每 50 ms 推一次 RMS 电平数组（Tauri event），前端 Canvas 渲染。
- VAD 有两条配置路径：能量模式按 30 ms 帧计算 RMS，严格使用 `RMS > energy_threshold`；神经网络模式使用 `silero-vad-crs 0.4` 的完整 f32 内嵌权重与兼容 SSE/NEON 自动加速，无 ONNX Runtime、外部 DLL 或运行时下载，每 512 个 16 kHz 样本推理。神经网络进入门限使用用户值，退出门限 `max(threshold - 0.15, 0.01)`，最短语音 90 ms、结束静音 100 ms；初始化或推理失败降级到同一快照中的能量门限。
- 两条 VAD 都用于结束裁剪和长录音静音切片：首个语音区间前保留 300 ms，最后语音区间后保留 150 ms。未检出语音但存在连续至少 90 ms 弱信号时提交未裁剪 16 kHz 音频，真正静音返回 `NoSpeech`。录音开始时快照 VAD 配置并随 `Recording` 保存，失败重试与长录音切片不得重读实时设置。
- 设备选择：空配置使用系统默认输入设备并在下次开流时跟随系统切换；设置中固定设备时，Windows 保存 WASAPI `IMMDevice` endpoint ID、界面只显示 friendly label。固定 ID 缺失必须返回 `AudioDevice`，禁止静默回退默认设备。历史版本保存的 display name 仅在当前枚举结果中唯一匹配时迁移为 ID；无匹配或同名歧义均明确失败。
- 录音启动失败与运行时 stream error/设备拔出通过独立 `RecordingFailed` 事件主动通知 orchestrator；`Recording` 立即转 `Failed(Recording)` 并执行 `CancelRecording` 释放 active stream，不等待下一次 stop。该失败态的重试语义是重新打开麦克风并开始录音，不进入注入或 STT 阶段。
- 松键后 orchestrator 先发布 `Transcribing` 快照，再把 `AudioService::stop` 放入阻塞任务；只有同一活动会话收到 `RecordingFinished` 才保存音频并启动 STT。音频收尾期间主循环继续处理 Esc；取消先到时，完成结果直接丢弃且不得调用 Provider。

### 7.5 文本注入（Injector 后端链）

```
trait Injector { fn inject(&self, text: &str, target: &FocusInfo) -> Result<()>; }
```

会话注入另有可取消入口，返回 `Injected | Cancelled`，并共享原子门闩 `Pending | Cancelled | Committed`。paste 后端可在写入临时剪贴板后保持 Pending，紧邻首个真实 Cmd/Ctrl+V 或逐字输入 OS API 前竞争提交：取消先赢必须恢复剪贴板且不发输入，提交先赢则取消不再生效并正常完成。非会话调用继续使用原有兼容入口。

按平台组成后备链（每个后端失败自动尝试下一个，全失败 → 复制到剪贴板 + HUD 提示）：

- macOS: `paste`（保存剪贴板 → 写入 → CGEvent Cmd+V → 延迟 → 恢复）
- Windows: `paste`（原生 SendInput Ctrl+V，校验返回发送计数）；备选 `type_direct`（SendInput `KEYEVENTF_UNICODE`，按 UTF-16 code unit 发送完整 down/up，正确处理 surrogate pair、emoji 与换行）。无焦点或 UIPI 阻止时只复制并返回明确分类，不自动提权
- Linux X11: `paste`（XTEST）
- Linux Wayland: 探测 compositor → wlroots 系用 `wtype`；GNOME/KDE 用 `dotool`/`ydotool`（需 /dev/uinput 权限，设置页内嵌引导）；全不可用 → 「已复制，请 Ctrl+V」模式

### 7.6 读取选中文本（Selection 降级链）

1. macOS：`AXUIElement.kAXSelectedTextAttribute` → 失败：临时静音系统提示音 + CGEvent Cmd+C + 读剪贴板 + 恢复（禁止通过 enigo/rdev 查询输入法布局，避免 HIToolbox 主队列断言）；**AX 属性可读但为空 = 明确无选区，直接返回、不走剪贴板降级**（降级只给不支持 AX 的应用）；定位用 `AXSelectedTextRange` + `AXBoundsForRange`，不可得时回退居中。
2. Windows：专用 COM worker 上执行 UIA `TextPattern.GetSelection()`；可读但为空表示明确无选区，不走剪贴板；不支持 TextPattern、COM 错误或超时才走 Ctrl+C 降级。bounds 使用 UIA range bounding rectangles，并统一转换到目标 monitor 坐标。
3. Linux：X11 primary selection 直读；Wayland 下 primary selection 可用则用，否则明确降级提示。
4. 降级链每步有 300 ms 超时。Windows Ctrl+C 降级先写 sentinel，再以 clipboard sequence 变化确认复制发生；文本与已知 metadata 必须在同一次打开剪贴板期间读取。部分应用（如 JetBrains IDE）会为同一次复制分批发布多个格式并连续推进 sequence：轮询与原子读取之间 sequence 不同时，只有二者的非空 clipboard owner HWND 相同才接受载荷，并以读取到的最终 sequence 作为恢复基线；owner 变化仍按用户或剪贴板管理器中途修改处理，禁止消费或覆盖。VS Code/Monaco 的 `vscode-editor-data` 只有在大小受限、JSON 合法、`version=1` 且 `isFromEmptySelection` 为布尔 `true` 时才证明无选区；格式缺失、损坏、超大、字段缺失、未知版本或未知编辑器都保守保留文本，禁止按长度或换行猜测。原剪贴板的可恢复 HGLOBAL 格式统一快照，且仅当读取后的 sequence 未再变化时恢复，不能覆盖用户或剪贴板管理器的中途修改。无可信空选区 marker、又把无选区 Ctrl+C 定义为复制整行的第三方编辑器仍是已知残余边界。
5. **读取时机 = 触发键松开、调 STT 的同时（并发执行，不增加延迟）**——绝不能在触发键按住期间执行：剪贴板降级要模拟 Cmd/Ctrl+C，合成的普通键 down 会命中「组合键让路」规则、把当前会话静默取消。选区在录音期间不会变（HUD 不抢焦点），松开后读取语义等价。

## 8. Linux/Wayland 支持策略（明确的分级承诺）

**最低系统版本**：macOS 12+（Monterey）；Windows 10 22H2+ / Windows 11 x64（MSVC ABI）；Linux 需 webkit2gtk-4.1（Ubuntu 22.04+ / Debian 12+ / Fedora 38+）。Windows 安装器为 NSIS x64，并携带 WebView2 Evergreen Bootstrapper，在缺失 WebView2 时联网安装；本地推理依赖的 sherpa/ONNX、VC++ 2015-2022 x64 runtime 与 Vulkan loader 均 app-local 随包分发，不要求用户另装开发工具或系统级 VC/Vulkan runtime。

| 级别 | 环境 | 承诺 |
|---|---|---|
| Tier 1 | Ubuntu 22.04+ X11、Fedora X11 | 全功能 |
| Tier 2 | KDE Wayland、GNOME ≥ 48 Wayland、Hyprland | 全功能，但需一次 Portal 授权；HUD 在 GNOME 降级为托盘状态指示 |
| Tier 3 | 其他 Wayland | 快捷键走 CLI/evdev、注入走 ydotool 或剪贴板模式；文档明示 |

在 README 与设置页「诊断」标签中内置**环境自检**（显示当前探测到的 compositor、可用后端、缺失权限），把 Linux 的碎片化变成可见、可自助解决的问题。

## 9. 配置、密钥与磁盘数据布局

- **网络代理**：所有 Provider 请求经统一的 reqwest 客户端工厂，默认**跟随系统代理**；设置-通用可改为「手动代理」（HTTP/SOCKS5，host:port + 可选认证）或「直连」。目标用户中访问国际端点需代理者众多；代理凭据若有同样按敏感配置字段处理，导出与日志必须脱敏。
- 设置文件：`tauri-plugin-store`（JSON，位于平台标准配置目录），带 `schema_version` 与迁移函数。
- `hotkeys.dictation` / `hotkeys.assistant` / `hotkeys.translation` 分别保存规范化完整 chord。schema v6 将历史后端/浏览器别名迁移为 §7.3 的稳定 `KeyId`；schema v8 升级时把 v7 翻译键按旧规则重建为听写与助手的有序去重并集，此后每组独立归一化、持久化，修改任一项不再派生其他项。
- schema v7 增加 `dictation.vad.{mode,energy_threshold,neural_threshold}`；旧配置迁移为神经网络模式。门限保存前必须为有限值且在规格范围内；加载时 VAD 子树无效只恢复该子树，不得丢弃其他设置。
- `dictation.microphone`：空字符串表示系统默认；非空值表示平台稳定设备 ID（Windows 为 WASAPI endpoint ID）。旧版设备名称在运行时唯一匹配后写回稳定 ID。
- 密钥：随 profile 的 `credentials` 字段存入 `settings.json`，与其他配置项同路径；诊断包导出清空 `credentials`，日志 redact 层拦截 Authorization/密钥文本。旧版 `keyring://` 引用在迁移时清除，运行时也视为未配置。
- 日志：`tracing` + 滚动文件，默认 INFO；**全局 redact 层**保证 Authorization/密钥永不入日志（见 §5.5）。

运行时磁盘布局：

| 内容 | 位置（平台标准目录） | 说明 |
|---|---|---|
| `settings.json` | config dir（如 `~/Library/Application Support/ink.typex.app/`） | 含 profile 与 credentials；导出诊断时 credentials 为空 |
| `history.sqlite` | data dir | WAL 模式；启动时跑保留期清理；成功助手问答复用现有行结构保存 `mode=assistant`、语音指令与完整回答，统计查询排除所有 `assistant` 行 |
| 失败重试音频 | cache dir `/pending/` | 会话结束/放弃即删；启动时清孤儿文件 |
| 日志 | log dir，按天滚动，保留 7 天 | 经 redact 层 |

应用标识符：`ink.typex.app`（与域名 typex.ink 对应）。

## 10. IPC 契约（tauri-specta 单一事实源）

所有 command/event 在 Rust 侧定义，`pnpm gen:ipc` 生成 `src/ipc/bindings.ts`；前端禁止手写 `invoke("...")` 字符串。

### 10.1 Commands（前端 → Rust）

| 分组 | Command | 说明 |
|---|---|---|
| 会话 | `cancel_session` / `retry_session` / `dismiss_session` | HUD 按钮；toggle 录音的开始/停止走快捷键，不提供 command（避免两套触发路径） |
| 配置 | `get_settings` / `update_settings(patch)` | patch 语义，返回完整新配置；F-10 词典作为 `Settings.dictionary.terms[]` 随设置同步 |
| 音频 | `list_audio_devices` | 返回 `Result<AudioInputDevice[]>`；每项为 `{ id, label }`，设置界面展示 label、保存 id，枚举失败不得伪装为空列表 |
| Profile | `list_profiles` / `upsert_profile` / `delete_profile` / `activate_profile { slot, id }` / `set_profile_secret` / `test_profile { id }` | `profiles[]` 是全局服务配置池；`activate_profile` 只改功能槽位指针并校验 STT/LLM 能力兼容；密钥字段由 `set_profile_secret` 写入 profile credentials |
| 本地模型 | `list_local_models` / `download_local_model { model_id, source? }` / `cancel_local_download { model_id }` / `delete_local_model { model_id, force }` / `get_hardware_tier` | `source` 为空时使用 settings.general.model_download_source；固定源只在模型管理页底部配置 |
| 窗口 | `complete_onboarding` / `open_onboarding_window` / `set_hud_size { width, height }` | `complete_onboarding` 先创建/显示并聚焦主页，再关闭 onboarding；任一步失败返回错误并保留可重试的引导页。HUD 仅上报内容逻辑尺寸，由 Rust 在目标屏工作区以单次原生 frame 更新原子缩放并重新居中 |
| 助手窗口 | `assistant_window_ready` | assistant WebView 注册完 `assistant://*` 监听器后上报；后端首次创建窗口时等待它，避免首轮 `assistant://started` 丢事件 |
| 快捷键 | `begin_hotkey_capture` / `end_hotkey_capture` | 录制模式：期间原始按键流经 event 上报 |
| 历史 | `query_history { search, offset }` / `get_stats` / `delete_history_item` / `clear_history` | `get_stats` 返回主页统计（总时长/字数/节省时间/语速，本地聚合） |
| 系统 | `get_permission_status` / `open_permission_settings { kind }` / `get_diagnostics` / `set_paused(bool)` / `copy_last_result` / `check_update` | `check_update` 按 `settings.general.update_channel` 选择 stable/nightly 更新源；字段缺省时由编译版本决定（prerelease → nightly，纯 SemVer → stable） |

### 10.2 Events（Rust → 前端）

| Event | 载荷 | 消费者 |
|---|---|---|
| `session://snapshot` | `SessionSnapshot`（phase、mode、时长、错误码…） | HUD、托盘 |
| `session://audio-level` | `f32[]`（50ms 节流） | HUD 波形 |
| `assistant://started` | `{ request_id, instruction, selection_chars? }` | 回答弹窗：重置内容 + 指令回显 |
| `assistant://delta` | `{ request_id, text_delta }` | 回答弹窗流式渲染 |
| `assistant://done` | `{ request_id, full_text }` | 回答弹窗终态（改写型结果不经 assistant:// 事件，走 session 注入） |
| `assistant://error` | `{ request_id, error }` | 回答弹窗错误展示（仅弹窗已呼出后的流中断；此前的失败走 HUD） |
| `settings://changed` | 变更后的 `Settings` | 全窗口 |
| `hotkey://captured` | 录制期间的按键组合 | HotkeyRecorder 控件 |
| `permission://changed` | `PermissionStatus` | onboarding、诊断页 |
| `update://available` | 版本信息 | 设置-关于 |

命名规范：`域://kebab-case`；载荷全部为 `types/` 中的 struct，禁止匿名对象。

## 11. 前端（Vue 3）结构

```
src/
├── windows/                  # 每窗口一个 Vite HTML 入口（vite 多页构建）
│   ├── hud/        main.ts + Hud.vue
│   ├── assistant/  main.ts + Assistant.vue
│   ├── settings/   main.ts + Settings.vue + pages/（General/Dictation/…/Diagnostics/Debug）
│   ├── onboarding/ main.ts + 5 个 Step 组件
│   └── home/       main.ts + Home.vue + pages/（Overview/History；侧边栏布局）
├── components/               # 04 章组件库：Button/Input/SecretInput/Select/Toggle/
│                             # HotkeyRecorder/ProviderCard/Waveform/StatusPill/Kbd/…
├── composables/              # useSession()（订阅 session://）、useSettings()、
│                             # useAudioLevel()、useIpcEvent(name, cb)
├── stores/                   # Pinia：settings.ts / session.ts / assistant.ts / history.ts
├── ipc/bindings.ts           # tauri-specta 生成物（gitignore，CI 校验新鲜度）
├── i18n/                     # zh-CN.json / en.json（含全部错误码文案）
├── styles/tokens.css         # 04 章 design tokens 唯一落地处
└── shared/                   # 纯工具（无 Tauri 依赖，可被 vitest 直接测）
```

要点：

- **HUD 极简纪律**：hud 入口只允许依赖 `StatusPill`/`Waveform`/`useSession`，禁止引入 Pinia、路由、Markdown 渲染等重依赖——保证隐藏时零活动、显示时瞬时渲染（§12 性能预算）。打包时 hud chunk 单独产出并在 CI 里做体积断言（< 150 KB gzip 前端资产）。
- **HUD frame 所有权**：前端 `ResizeObserver` 只合并、串行上报最新内容尺寸；屏幕选择、DPI 换算、缩放与重新居中统一由 `app/windows.rs` 完成。macOS 使用单次 `NSWindow.setFrame`，Windows 使用单次 `SetWindowPos`，不得由前端分别调用 `setSize` / `setPosition`。
- 状态所有权：**业务状态（会话、配置）的真身在 Rust**，Pinia 只是订阅缓存 + 表单草稿；前端不自行推导会话状态。
- Markdown 渲染（仅 assistant 窗口）：`markdown-it` + 白名单 sanitize；LLM 输出视为不可信内容，禁 raw HTML。
- i18n：`vue-i18n`，key 与 Rust `ErrorCode` 对齐（`error.auth_error` …）。

## 12. 性能预算（发布门槛，CI 中可测项自动化）

| 指标 | 预算 |
|---|---|
| 安装包体积 | ≤ 25 MB（不含可选本地模型） |
| 空闲内存（全进程合计） | ≤ 150 MB（目标 100 MB） |
| 空闲 CPU | ≈ 0%（无常驻动画/轮询；键盘监听线程阻塞式） |
| 按键到 HUD 出现 | ≤ 100 ms |
| 按键到录音实际开始 | ≤ 150 ms（cpal 流预热：常驻保持打开或首按预热，实测决定） |
| 松开到上屏（5s 中文短句，快 Provider） | P50 ≤ 1.5 s |
| 冷启动到托盘就绪 | ≤ 2 s |

## 13. 测试策略

> 概览如下；**完整的可执行规范（场景清单、mock 基座、覆盖率门槛、CI 流水线、AI 开发纪律）以 [07 测试规范](07-testing.md) 为准**。

| 层 | 工具 | 覆盖重点 |
|---|---|---|
| 状态机单测 | `cargo test`（纯函数，无 IO） | §5.2 全部转移规则：长短按、组合键让路、重按忽略、失败重试、session_id 竞态 |
| Provider 集成测 | `wiremock`（本地 mock HTTP） | 两种 wire 格式的请求构造/SSE 解析/错误分类/重试；火山 flash 的 header 判定 |
| 整理提示词回归 | `docs/fixtures/denoise-cases.md` 驱动的手动/半自动评测 | 改口/语气词/格式指令样例集，防提示词回归；自动评测器必须复用运行时提示词，不维护单独提示词副本 |
| 前端组件 | `vitest` + `@vue/test-utils`（IPC 层 mock bindings） | HotkeyRecorder、ProviderCard 表单、错误文案映射 |
| 端到端 | 用**人工回归清单**（`docs/09-release-checklist.md`，按 02 章验收标准逐条 × 三平台） | 键盘监听/注入/权限这类系统能力自动化成本过高，不强上 E2E 框架 |
| CI 门槛 | clippy `-D warnings`、`cargo test`、`vue-tsc`、vitest、bindings 新鲜度校验、hud 体积断言 | PR 必须绿 |

## 14. 编码、CI 与协作规范

- **Rust**：Edition 2024；`rustfmt` 默认配置；公共 API 必须有 doc comment；`unsafe` 仅允许出现在 `platform/` 或明确命名的平台 backend（如 `hotkey/windows_backend.rs`、`inject/windows.rs`、`selection/windows.rs`），并逐处注释理由与系统不变量。
- **前端**：ESLint + Prettier；组件 `<script setup lang="ts">`；样式只用 Tailwind 类 + tokens.css 变量，禁止组件内硬编码色值（stylelint 规则）。
- **命名**：产品名 Typex（[ADR-14](08-decisions.md)）；bundle id `ink.typex.app`；crate/npm 包名 `typex`。
- **Windows 本地 Tauri 构建**：`pnpm tauri dev/build/bundle` 统一经 `scripts/tauri.mjs` 启动。包装器只为会编译原生代码的 `dev` / `build` 命令通过 `vswhere` 初始化 MSVC x64 环境，并发现、校验 Vulkan SDK / SPIRV-Headers CMake package；`bundle` 只打包已有 EXE，原样透传官方 Tauri CLI。Cargo 产物仍使用项目默认 `src-tauri/target`，禁止重定向 `CARGO_TARGET_DIR` 或占用应用运行时 cwd。仓库级 Cargo config 只为 `x86_64-pc-windows-msvc` 固定 Ninja，并为所有 Cargo 入口注入 llama.cpp CMake project include；后者仅把 Vulkan shader generator 的 `ExternalProject` 根目录收敛到同一 Cargo `OUT_DIR` 下的 `build/ep`，避免上游默认嵌套路径超过 MSVC 限制。`llama-cpp-sys-2` 与 `sherpa-rs-sys` 的 dev package profile 关闭 debug assertions，使其 build script 与实际使用的 Release 原生库保持一致，避免错误引入 debug CRT。Windows 的 sherpa-onnx 使用上游 `download-binaries` 固定版本预编译包和内置 SHA-256 校验，避免本地 FetchContent 生成超长目录；macOS/Linux 保持原 generator 与源码构建，包装器原样透传官方 Tauri CLI。
- **CI**：GitHub Actions 至少覆盖 macOS 与 Windows x64，Linux 后端落地后加入 Linux x64；Windows 同时运行 default / `--no-default-features` 的 check、clippy、test，以及前端 build/test 和 NSIS package smoke。Windows 还必须构建 debug EXE 并验证 PE subsystem 为 GUI。NSIS smoke 必须确认默认目录 hook 已进入渲染脚本，解包安装器，核对 app-local DLL/许可/manifest 的文件名与 SHA256，并检查 release/包内 EXE 的 GUI subsystem 与 PE import 闭包，不能只断言安装器文件存在。
- **发布**：tag → 各平台 build job 产出唯一命名的平台 artifact 与 updater manifest fragment → publish job 校验后聚合 SHA256 和唯一 `latest.json`。Windows 使用 Tauri 2 原生 updater 格式：同一个 NSIS x64 `.exe` 同时作为手动安装和 updater 下载资产，旁边发布对应 `.exe.sig`，不生成仅供 Tauri v1 迁移的 legacy `.nsis.zip`；Windows 与 macOS 复用仓库现有 updater 密钥。构建前先从原生 release 产物、Microsoft VC tools redist 和固定版本/哈希的 Vulkan Loader 包生成 runtime staging，Tauri Windows resource map 将其安装到 EXE 同目录。Tauri `.sig` 是更新完整性签名，Authenticode 是 Windows 发布者身份，二者独立。SignPath/Authenticode 可后置，但构建链必须保留稳定签名插入点。更新通道分 stable/nightly，任何平台 fragment 不得同名覆盖或跨通道引用。
- **提交**：Conventional Commits（`feat(audio): …`，scope 用本章模块名）；分支 `feat/…`、`fix/…`；PR 模板含「影响的设计书章节」栏——**代码与文档不同步的 PR 不合**。
- **文档同步纪律**：改动 IPC 契约、配置 schema、状态机行为时，必须同 PR 更新本章或对应章节。

## 15. 完整平台适配的实施顺序

以下切片只用于尽早验证高风险系统链路，不是独立交付物或支持范围缩减；平台适配必须继续完成本章全部能力与 [09 发布人工回归清单](09-release-checklist.md)。

1. 仓库骨架 + CI（目录树按 §4 建模块，先跑通最小构建）。
2. `types/` + `error.rs` + `settings/schema.rs`（数据先行，IPC 生成链路跑通）。
3. hotkey（仅 rdev 单键）→ audio（录到 WAV）→ providers/stt/openai_compat（写死配置）→ inject/paste：串成第一条能跑的听写管线，作为最小可运行切片验收。
4. orchestrator 状态机替换掉临时串联代码 → HUD 接 `session://snapshot`。

后续进度使用 GitHub Issues / Projects / Milestones 管理，模块归属以本章为准。
