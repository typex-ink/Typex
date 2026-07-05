# 07 · 代码架构与落地计划

> Typex 产品设计书 · 第七章
> 本章是完整的技术落地计划：用什么技术（§1–§2）、代码怎么组织（§3–§5）、各平台系统能力怎么实现（§6–§9）、契约与前端（§10–§11）、性能与质量门槛（§12–§14）、从哪开始写（§15）。开发中若与本章冲突，先改文档再改代码。
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

**前端：Vue 3 + TypeScript + Tailwind CSS v4**（已定，[ADR-6](09-decisions.md)）。
组合式 API + `<script setup>`；状态管理用 Pinia；构建走 Vite（Tauri 默认脚手架）。参考实现提示：同类项目 Whispering（Svelte）与 Handy（React）的**架构思路**均可借鉴，组件代码需按 Vue 重写。
IPC 使用 **tauri-specta** 自动生成 TS 类型绑定，杜绝前后端接口漂移。波形绘制用 Canvas 2D（60 fps 足够，无需 WebGL）。

**Rust 关键依赖**（2026-07 均为活跃维护状态）：

| 用途 | crate | 备注 |
|---|---|---|
| 全局按键监听（push-to-talk） | `rdev`（mac/win/X11）+ `ashpd`（Wayland Portal）| Handy 同款路线；支持单修饰键按住 |
| 录音 | `cpal` | 事实标准 |
| 重采样 | `rubato` | 设备原生采样率 → 16 kHz mono |
| VAD | `vad-rs`（Silero） | 静音裁剪 + 长录音切片边界 |
| WAV 编码 | `hound` | 上传用 |
| 提示音 | `rodio` | 开始/结束/错误音 |
| 文本注入 | `enigo`（模拟 Cmd/Ctrl+V）+ `arboard`（剪贴板存取恢复） | Wayland 另有外部工具后端，见 §8 |
| 读取选中文本 | `get-selected-text`（AX → 剪贴板降级）；必要时直接用 `axuielement` / `uiautomation` | |
| HTTP / SSE | `reqwest` + `eventsource-stream` | 全部 Provider 调用 |
| WebSocket | `tokio-tungstenite` | 火山流式（P2） |
| 密钥存储 | `keyring` | Keychain / Credential Manager / Secret Service |
| 本地数据 | `rusqlite`（历史）+ `tauri-plugin-store`（设置） | |
| macOS 权限 | `tauri-plugin-macos-permissions` + `macos-accessibility-client` | 检测 + 引导跳转 |
| 应用基建 | `tauri-plugin-single-instance` / `-autostart` / `-updater` / `-global-shortcut`（备用）；托盘内置 | 全官方插件 |
| Linux HUD | `gtk-layer-shell`（经 `gtk_window()` 句柄） | Handy 验证过的方案 |
| 本地推理（v1.1，[ADR-20](09-decisions.md)/[ADR-22](09-decisions.md)） | LLM+Qwen3-ASR：`llama-cpp-2`；轻量档 STT：`sherpa-onnx`（官方 crate，静态链接）；硬件探测：`sysinfo` | 不进 v1.0 依赖树；feature flag `local-models` 隔离，模型按硬件档位下载（[03 §8](03-model-providers.md)）；whisper.cpp 降为可选扩展 |

## 2. 进程与窗口模型

单进程（Tauri 主进程 = Rust 核心），多 WebView 窗口按需创建/销毁：

| 窗口 | 生命周期 | 特性 |
|---|---|---|
| **HUD** | 常驻（隐藏/显示切换，避免创建延迟） | 无边框、透明、置顶、**不可获得焦点**（macOS 必须用 NSPanel/nonactivating，否则注入目标失焦——经 `tauri-nspanel`）、忽略鼠标事件（除按钮区） |
| **回答弹窗** | 按需显示，关闭即隐藏 | 无边框、透明、禁用原生窗口阴影、不可手动调整大小、原生窗口高度跟随内容、置顶、可获得焦点、优先贴近选区下方、失焦自动关闭（无 pin）、只读展示（无输入能力，见 05 §4） |
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
├── src-tauri/                    # Rust（单 crate，v1 不拆 workspace，见 §5.6）
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
│       │   │   └── local.rs      # v1.1：Qwen3-ASR(llama.cpp) + SenseVoice(sherpa-onnx)（feature "local-models"）
│       │   ├── llm/
│       │   │   ├── mod.rs        # trait LlmProvider + PromptKit（内置提示词模板）
│       │   │   ├── chat_completions.rs
│       │   │   ├── responses.rs
│       │   │   └── local.rs      # v1.1：llama.cpp + Qwen3.5（仅整理/翻译槽，feature "local-models"）
│       │   └── models/
│       │       └── mod.rs        # v1.1：模型库清单 + 下载管理器 + 硬件分档探测（[03 §8]）
│       ├── settings/
│       │   ├── mod.rs            # SettingsService：读写、校验、变更广播
│       │   ├── schema.rs         # 全部配置结构体 + 默认值 + schema_version
│       │   ├── migrate.rs
│       │   └── secrets.rs        # keyring 封装（keyring:// 引用解析）
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
            ┌──────── cancel (Esc) ────────┐
            ▼                              │
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

- 状态机本体是**纯函数式转移表**（`fn advance(state, event) -> (state, Vec<Effect>)`），不做 IO；`Effect`（StartRecording / CallStt / Inject / EmitUi / PlayChime…）由 orchestrator 的执行器逐个 dispatch 到 service。**这是全项目单测密度最高的地方**（重按忽略、Esc 取消、组合键让路、失败重试等规则全部在此验证）。
- 任何时刻只有一个活动会话（v1 不并行）；每个会话有自增 `session_id`，所有异步回调带 id 校验，杜绝「上一条的转写结果注入到下一条」的竞态。
- 每次 phase 变更 → `SessionSnapshot` 经 `app/events.rs` 推送给前端（HUD/面板据此渲染，前端无自己的业务状态机；每个状态携带可显示的进度语义，见 [05 UX 规格 §3](05-ux-spec.md)）。

### 5.3 线程与异步模型

| 执行体 | 形态 | 职责 |
|---|---|---|
| Tauri 主线程 | 事件循环 | 窗口/托盘/菜单（部分平台 API 要求主线程） |
| tokio runtime | Tauri 自带 | orchestrator 执行器、全部 provider HTTP、settings/history IO |
| hotkey 线程 | `std::thread`（rdev 阻塞监听） | 键盘事件 → 判定逻辑 → `mpsc` 发给 orchestrator |
| cpal 回调线程 | 音频驱动回调 | **只做** ring buffer 写入（实时线程禁止分配/锁/日志） |
| audio worker 线程 | `std::thread` | 从 ring buffer 取样 → 重采样/电平/VAD → 电平事件节流 50ms 发前端 |

跨线程通信统一 `tokio::sync::mpsc`（service → orchestrator）与 `watch`（配置广播）；hotkey/audio 线程持有的发送端是它们与外界的唯一接口。

### 5.4 错误处理

- 底层各 service 用 `thiserror` 定义局部错误；顶层统一收敛为 `TypexError`，其中带 `ErrorCode`（`auth_error` / `network_error` / `timeout` / `rate_limited` / `server_error` / `no_speech` / `no_focus` / `permission_missing` / …）。
- `ErrorCode` 即 [05 §9 错误文案表](05-ux-spec.md) 的键：Rust 只发 code + 参数，**文案在前端 i18n 里**（中英双语一处维护）。
- 重试策略集中在 `providers/http.rs`（对 `rate_limited/server_error/network` 指数退避 ×2），orchestrator 不重复实现。

### 5.5 日志

`tracing` + `tracing-appender` 滚动文件（默认 INFO，诊断页可临时调 DEBUG）。全局 redact 层：`Authorization`、`X-Api-Access-Key`、任何 `credentials` 字段、以及**转写文本本身**（日志只记长度与耗时，不记内容——隐私承诺的一部分）。

### 5.6 关于不拆 workspace 的说明

v1 保持单 crate：模块边界靠 §3 依赖规则约束（CI 中可用 `cargo-modules`/review 把关），避免过早抽象。若后续出现「providers 想独立发布」或编译时间失控，再拆 `typex-core` / `typex-providers`——目录结构已按可拆分的形状组织。

## 6. 数据流与隐私边界

```
麦克风 ──▶ Rust 内存（PCM）──▶ 用户配置的 STT 端点（HTTPS）
                                    │
   选中文本（AX/剪贴板）─────────────┼──▶ 用户配置的 LLM 端点（整理/翻译/问答，HTTPS）
                                    ▼
                              注入目标应用
```

- **除用户自己配置的端点外，无任何网络请求**（更新检查除外，可关）。无遥测（见 [D-11](09-decisions.md)）。
- 音频不落盘；仅当转写失败等待重试时写入临时目录（加密与否 v1 从简：内存优先，超大切片才落盘），成功或放弃后立即删除。
- 剪贴板注入前保存原内容、注入后恢复；文档中明示「恢复仅支持文本/图片格式」的已知妥协。

## 7. 关键系统能力实现方案

### 7.1 平台适配矩阵（总览）

| 能力 | macOS | Windows | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| 全局按住说话 | rdev（需辅助功能/输入监听权限） | rdev（WH_KEYBOARD_LL） | rdev（XTEST/XRecord） | Portal GlobalShortcuts（ashpd）；evdev 兜底 |
| 文本注入 | 剪贴板 + CGEvent Cmd+V | 剪贴板 + SendInput Ctrl+V | 剪贴板 + XTEST Ctrl+V | wtype（wlroots 系）/ ydotool·dotool（GNOME/KDE）/ 仅复制降级 |
| 读选中文本 | AX API → 静音 Cmd+C 降级 | UIA TextPattern → Ctrl+C 降级 | primary selection | primary selection（部分可用）→ 仅手动粘贴降级 |
| HUD 置顶浮窗 | NSPanel（不抢焦点） | 原生支持 | 原生支持 | gtk-layer-shell（GNOME 不支持 → 降级为托盘状态） |
| 托盘/自启/单实例 | ✅ | ✅ | ✅ | ✅（托盘图标需落盘 XDG_RUNTIME_DIR 等已知小坑） |

### 7.2 已知平台坑清单（开发时逐条对照）

1. **macOS 权限静默失效**：未授权辅助功能时 `rdev::listen` 静默无事件、不报错——必须用 `macos-accessibility-client` 主动检测并引导；开发时给终端/IDE 授权。
2. **macOS 签名后麦克风弹窗 bug**（tauri#9928/#11951）：Info.plist 有 `NSMicrophoneUsageDescription` 也可能不弹授权——需原生侧主动 `AVCaptureDevice.requestAccess`（`tauri-plugin-macos-permissions` 已封装）；entitlement `com.apple.security.device.audio-input`。
3. **HUD 抢焦点会毁掉注入**：macOS 必须 NSPanel + nonactivating style；其他平台设置不可聚焦标志。
4. **逐字模拟键入在非美式布局/输入法激活时乱码** → 默认剪贴板粘贴路径；粘贴前 60 ms 级可调延迟（部分慢应用需要）。
5. **X11 组合键 release 事件顺序 bug**（global-hotkey#39）→ 用 rdev 自维护按键状态，不依赖热键 API 的 release。
6. **Windows UIPI**：目标窗口是管理员进程时 SendInput 被拦截 → 检测失败并提示（或建议以管理员运行）。
7. **webkit2gtk**：NVIDIA 驱动下白屏/崩溃 → 启动时探测并自动注入 `WEBKIT_DISABLE_DMABUF_RENDERER=1`；仅支持 webkit2gtk-4.1 的发行版（Ubuntu 22.04+）。
8. **剪贴板恢复不保真**（arboard 仅文本/图片）→ 设置页明示；与剪贴板管理器可能互相干扰记录中间内容。

### 7.3 快捷键（push-to-talk 细节）

- 不用 `tauri-plugin-global-shortcut` 作为主路径（无法监听单个修饰键；X11 release 有 bug），改用 **rdev 独立线程**自维护 down/up 状态——默认键位为全修饰键三角方案（右⌘/右⌥ 及其组合，见 [05 §7.1](05-ux-spec.md)），必须支持单修饰键触发。
- **组合键让路规则（核心）**：触发键按住期间收到任何**普通键** down 事件 → 判定用户在使用系统组合键（`⌘C`、`AltGr+E` 等），立即静默取消本次录音、不产生任何输出、按键完全放行。监听是 listen-only，普通键本来就不被拦截，此规则只是状态机层面的取消逻辑。
- **唯一需要事件拦截的点**：Windows 上单击 Alt 会聚焦菜单栏——助手键（右 Alt）短按时用低级钩子 `WH_KEYBOARD_LL` 吞掉对应 keyup。macOS/Linux 的修饰键单按无系统副作用，无需拦截。启动时自检钩子可用性，失败降级为「监听不拦截」+ UI 提示改键。
- 长按/短按判定：press 后 350 ms 内 release = toggle（三种模式一致，含助手）；超过 = push-to-talk（release 即停止）。**乐观启动**：触发键按下即开始录音，判定窗口内组合出第二触发键则无缝切换为翻译模式，音频保留。
- Wayland：探测 `XDG_SESSION_TYPE`；优先 `ashpd` 走 `org.freedesktop.portal.GlobalShortcuts`（KDE/GNOME≥48/Hyprland 支持，Activated/Deactivated 信号天然支持按住；注意 Portal 快捷键由 compositor 分配，未必能绑到「单独的右⌥」，此时默认键退化为 compositor 允许的组合键）；不可用时提示 evdev 方案（用户加入 `input` 组）或 compositor 绑定 `typex toggle` CLI 命令（经 single-instance 转发）。

### 7.4 录音

- cpal 以设备原生采样率（44.1/48 kHz）开流；callback 内只做 ring buffer 拷贝，重采样（rubato → 16 kHz mono f32）与电平计算在 worker 线程。
- HUD 波形数据：每 50 ms 推一次 RMS 电平数组（Tauri event），前端 Canvas 渲染。
- VAD（Silero）用于：① 结束时裁剪首尾静音（省上传/省钱/提速）；② 长录音在静音处切片（F-1 无上限要求）。
- 设备选择：默认系统输入设备并跟随系统切换；设置中可固定设备。

### 7.5 文本注入（Injector 后端链）

```
trait Injector { fn inject(&self, text: &str, target: &FocusInfo) -> Result<()>; }
```

按平台组成后备链（每个后端失败自动尝试下一个，全失败 → 复制到剪贴板 + HUD 提示）：

- macOS: `paste`（保存剪贴板 → 写入 → CGEvent Cmd+V → 延迟 → 恢复）
- Windows: `paste`（SendInput Ctrl+V）；备选 `type_direct`（SendInput KEYEVENTF_UNICODE，无剪贴板污染，用户可选）
- Linux X11: `paste`（XTEST）
- Linux Wayland: 探测 compositor → wlroots 系用 `wtype`；GNOME/KDE 用 `dotool`/`ydotool`（需 /dev/uinput 权限，设置页内嵌引导）；全不可用 → 「已复制，请 Ctrl+V」模式

### 7.6 读取选中文本（Selection 降级链）

1. macOS：`AXUIElement.kAXSelectedTextAttribute` → 失败：临时静音系统提示音 + CGEvent Cmd+C + 读剪贴板 + 恢复（禁止通过 enigo/rdev 查询输入法布局，避免 HIToolbox 主队列断言）；**AX 属性可读但为空 = 明确无选区，直接返回、不走剪贴板降级**（降级只给不支持 AX 的应用）；定位用 `AXSelectedTextRange` + `AXBoundsForRange`，不可得时回退居中。
2. Windows：UIA `TextPattern.GetSelection()` → 失败：Ctrl+C 降级。
3. Linux：X11 primary selection 直读；Wayland 下 primary selection 可用则用，否则明确降级提示。
4. 降级链每步有 300 ms 超时；剪贴板法须处理「无选中时复制整行」的误触（对比复制前后剪贴板内容 + 长度启发式）。
5. **读取时机 = 触发键松开、调 STT 的同时（并发执行，不增加延迟）**——绝不能在触发键按住期间执行：剪贴板降级要模拟 Cmd/Ctrl+C，合成的普通键 down 会命中「组合键让路」规则、把当前会话静默取消。选区在录音期间不会变（HUD 不抢焦点），松开后读取语义等价。

## 8. Linux/Wayland 支持策略（明确的分级承诺）

**最低系统版本（建议值，随 CI 实测修正）**：macOS 12+（Monterey，覆盖 Tauri 2 与 NSPanel 方案的舒适区）；Windows 10 1809+（WebView2 运行时要求，安装器自动引导安装 WebView2）；Linux 需 webkit2gtk-4.1（Ubuntu 22.04+ / Debian 12+ / Fedora 38+）。

| 级别 | 环境 | 承诺 |
|---|---|---|
| Tier 1 | Ubuntu 22.04+ X11、Fedora X11 | 全功能 |
| Tier 2 | KDE Wayland、GNOME ≥ 48 Wayland、Hyprland | 全功能，但需一次 Portal 授权；HUD 在 GNOME 降级为托盘状态指示 |
| Tier 3 | 其他 Wayland | 快捷键走 CLI/evdev、注入走 ydotool 或剪贴板模式；文档明示 |

在 README 与设置页「诊断」标签中内置**环境自检**（显示当前探测到的 compositor、可用后端、缺失权限），把 Linux 的碎片化变成可见、可自助解决的问题。

## 9. 配置、密钥与磁盘数据布局

- **网络代理**：所有 Provider 请求经统一的 reqwest 客户端工厂，默认**跟随系统代理**；设置-通用可改为「手动代理」（HTTP/SOCKS5，host:port + 可选认证）或「直连」。目标用户中访问国际端点需代理者众多，此项为 v1 必备；代理凭据同样入 keyring。
- 设置文件：`tauri-plugin-store`（JSON，位于平台标准配置目录），带 `schema_version` 与迁移函数。
- 密钥：`keyring` crate 存 OS 凭据库，settings.json 只存 `keyring://typex/<slot>/<profile-id>` 引用；导出配置时不含密钥。
- 日志：`tracing` + 滚动文件，默认 INFO；**全局 redact 层**保证 Authorization/密钥永不入日志（见 §5.5）。

运行时磁盘布局：

| 内容 | 位置（平台标准目录） | 说明 |
|---|---|---|
| `settings.json` | config dir（如 `~/Library/Application Support/ink.typex.app/`） | 无密钥明文 |
| 密钥 | OS 凭据库 | service=`typex`，account=`<slot>/<profile-id>/<field>` |
| `history.sqlite` | data dir | WAL 模式；启动时跑保留期清理 |
| 失败重试音频 | cache dir `/pending/` | 会话结束/放弃即删；启动时清孤儿文件 |
| 日志 | log dir，按天滚动，保留 7 天 | 经 redact 层 |

应用标识符：`ink.typex.app`（与域名 typex.ink 对应）。

## 10. IPC 契约（tauri-specta 单一事实源）

所有 command/event 在 Rust 侧定义，`pnpm gen:ipc` 生成 `src/ipc/bindings.ts`；前端禁止手写 `invoke("...")` 字符串。

### 10.1 Commands（前端 → Rust，v1 完整清单）

| 分组 | Command | 说明 |
|---|---|---|
| 会话 | `cancel_session` / `retry_session` / `dismiss_session` | HUD 按钮；toggle 录音的开始/停止走快捷键，不提供 command（避免两套触发路径） |
| 配置 | `get_settings` / `update_settings(patch)` | patch 语义，返回完整新配置 |
| Profile | `list_profiles` / `upsert_profile` / `delete_profile` / `activate_profile { slot, id }` / `test_profile { id }` | 密钥字段单独走 `set_profile_secret`（不随 profile JSON 往返） |
| 快捷键 | `begin_hotkey_capture` / `end_hotkey_capture` | 录制模式：期间原始按键流经 event 上报 |
| 历史 | `query_history { search, offset }` / `get_stats` / `delete_history_item` / `clear_history` | `get_stats` 返回主页统计（总时长/字数/节省时间/语速，本地聚合） |
| 系统 | `get_permission_status` / `open_permission_settings { kind }` / `get_diagnostics` / `set_paused(bool)` / `copy_last_result` / `check_update` |  |

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
│   ├── settings/   main.ts + Settings.vue + pages/（General/Dictation/…/Diagnostics）
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

> 概览如下；**完整的可执行规范（场景清单、mock 基座、覆盖率门槛、CI 流水线、AI 开发纪律）以 [08 测试规范](08-testing.md) 为准**。

| 层 | 工具 | 覆盖重点 |
|---|---|---|
| 状态机单测 | `cargo test`（纯函数，无 IO） | §5.2 全部转移规则：长短按、组合键让路、重按忽略、失败重试、session_id 竞态 |
| Provider 集成测 | `wiremock`（本地 mock HTTP） | 两种 wire 格式的请求构造/SSE 解析/错误分类/重试；火山 flash 的 header 判定 |
| 整理提示词回归 | `docs/fixtures/denoise-cases.md` 驱动的**手动/半自动**评测脚本（`scripts/eval-denoise.ts`，可选真实 API key 运行） | 改口/语气词/格式指令样例集，防提示词回归 |
| 前端组件 | `vitest` + `@vue/test-utils`（IPC 层 mock bindings） | HotkeyRecorder、ProviderCard 表单、错误文案映射 |
| 端到端 | v1 用**人工回归清单**（`docs/release-checklist.md`，按 02 章验收标准逐条 × 三平台） | 键盘监听/注入/权限这类系统能力自动化成本过高，不强上 E2E 框架 |
| CI 门槛 | clippy `-D warnings`、`cargo test`、`vue-tsc`、vitest、bindings 新鲜度校验、hud 体积断言 | PR 必须绿 |

## 14. 编码、CI 与协作规范

- **Rust**：Edition 2024；`rustfmt` 默认配置；公共 API 必须有 doc comment；`unsafe` 仅允许出现在 `platform/` 并逐处注释理由。
- **前端**：ESLint + Prettier；组件 `<script setup lang="ts">`；样式只用 Tailwind 类 + tokens.css 变量，禁止组件内硬编码色值（stylelint 规则）。
- **命名**：产品名 Typex（[ADR-14](09-decisions.md)）；bundle id `ink.typex.app`；crate/npm 包名 `typex`。
- **CI**：GitHub Actions 三平台矩阵（macOS universal、Windows x64、Linux x64 AppImage+deb+rpm）；`cargo clippy -D warnings`、`cargo test`、前端 `vue-tsc --noEmit` + `vitest`。
- **发布**：tag → 自动构建 + updater 清单 + GitHub Release；签名/公证凭据经 GitHub Secrets（Apple Developer + SignPath，[ADR-11](09-decisions.md)）。
- **提交**：Conventional Commits（`feat(audio): …`，scope 用本章模块名）；分支 `feat/…`、`fix/…`；PR 模板含「影响的设计书章节」栏——**代码与文档不同步的 PR 不合**。
- **文档同步纪律**：改动 IPC 契约、配置 schema、状态机行为时，必须同 PR 更新本章或对应章节。

## 15. M0 落地顺序（本章的最小可用切片）

1. 仓库骨架 + CI（目录树按 §4 建空模块，先全部 `todo!()`）。
2. `types/` + `error.rs` + `settings/schema.rs`（数据先行，IPC 生成链路跑通）。
3. hotkey（仅 rdev 单键）→ audio（录到 WAV）→ providers/stt/openai_compat（写死配置）→ inject/paste：串成第一条能跑的听写管线（对应 [06 路线图 M0](06-roadmap.md) 验收）。
4. orchestrator 状态机替换掉临时串联代码 → HUD 接 `session://snapshot`。

之后各里程碑按 [06 路线图](06-roadmap.md) 推进，模块归属以本章为准。
