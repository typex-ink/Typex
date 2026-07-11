# Vendored Rust 包

本目录保存通过 `src-tauri/Cargo.toml` 的 `[patch.crates-io]` 覆盖上游版本的 Rust 包。这里是 vendor 原因与 Typex 本地补丁的统一清单；包内靠近修改点的 `Typex patch` 注释只用于解释具体代码。

## 维护规则

- 新增或升级 vendor 包时，必须在同一提交中更新本文件。
- 每个条目必须写明上游包名、精确版本或 revision、保留 vendor 的原因、受影响平台，以及所有 Typex 修改过的文件和行为。
- 修改已有 vendor 包时，必须把新增、调整或删除的补丁同步到对应文件清单；不得只在 `Cargo.toml` 或源码注释中说明。
- 对照上游时，以对应版本的 crates.io 发布包或记录的 revision 为基线。Cargo 生成的清单、锁文件、换行差异，以及从上游仓库补入但未修改的许可证文件不算 Typex 补丁。
- 上游版本已包含全部所需修复时，应优先升级并移除本地覆盖，同时删除本文件中的对应条目。

## `rdev` 0.5.3

**上游基线：** crates.io `rdev` 0.5.3。

**保留原因：** macOS 26 会要求 TSM 输入法 API 在主线程执行。上游在 event tap 监听回调中生成按键字符名，间接调用 `TSMGetInputSourceProperty`，会触发队列断言并以 `SIGTRAP` 崩溃。Typex 的热键逻辑只消费 `Key` 枚举，不需要 `Event.name`。此外，系统因超时或用户输入禁用 event tap 后，上游监听不会主动恢复。

**Typex 修改：**

- [`rdev/src/macos/common.rs`](rdev/src/macos/common.rs)：监听回调不再调用 `create_string_for_key`，统一将 `Event.name` 设为 `None`，避免在监听线程访问 TSM 输入法 API。
- [`rdev/src/macos/listen.rs`](rdev/src/macos/listen.rs)：保存 event tap 引用；收到 `TapDisabledByTimeout` 或 `TapDisabledByUserInput` 时重新启用 tap，避免全局热键监听静默失效。

## `cpal` 0.16.0

**上游基线：** crates.io `cpal` 0.16.0。

**保留原因：** 上游 macOS CoreAudio 代码把会被系统写入的长度输出参数建模为不可变值，在 macOS 26 的 release/LTO 构建中属于未定义行为，设备枚举可能以 `SIGSEGV` 崩溃。Typex 还需要持久化稳定的 Windows 音频端点 ID，而上游 `DeviceTrait` 只暴露用户可读名称。

**Typex 修改：**

- [`cpal/src/host/coreaudio/macos/enumerate.rs`](cpal/src/host/coreaudio/macos/enumerate.rs)：将设备枚举使用的 `data_size` 改为可变输出参数，并向 CoreAudio 传递对应的可变指针。
- [`cpal/src/host/coreaudio/macos/mod.rs`](cpal/src/host/coreaudio/macos/mod.rs)：同样修正设备支持配置查询中的 `data_size` 可变性。
- [`cpal/src/traits.rs`](cpal/src/traits.rs)：为 `DeviceTrait` 增加 `id()`；默认回退到 `name()`，保持其他后端兼容。
- [`cpal/src/platform/mod.rs`](cpal/src/platform/mod.rs)：在平台统一 `Device` 包装层中把 `id()` 转发给具体后端。
- [`cpal/src/host/wasapi/device.rs`](cpal/src/host/wasapi/device.rs)：Windows WASAPI 后端通过 `IMMDevice::GetId` 返回稳定端点 ID，并用 RAII 释放 COM 分配的字符串。

## `sherpa-rs-sys` 0.6.8

**上游基线：** crates.io `sherpa-rs-sys` 0.6.8。

**保留原因：** 该版本早于上游对 `cargs` 过滤和 Windows 源码复制恢复能力的修复，并假设系统存在 WMIC、使用 Visual Studio CMake generator。Typex 的 Windows 常规构建使用官方预构建二进制，但此覆盖仍承载同版本的构建脚本修复，并用于需要源码构建的目标。

**Typex 修改：**

- [`sherpa-rs-sys/build.rs`](sherpa-rs-sys/build.rs)：
  - 按 Robocopy 约定把退出码 0 至 7 视为成功，其余退出码或无退出码视为失败。
  - 以 `CMakeLists.txt` 作为源码复制完成标记；自动删除并重建中断复制留下的不完整目录，复制后再次校验。
  - MSVC 构建显式禁用 Fortran 探测，避免 Eigen 的可选 BLAS 检查误选 PATH 中的 MinGW `gfortran`。
  - 从链接库和运行时资产中排除只供 sherpa 命令行示例使用的 `cargs`。
- [`sherpa-rs-sys/sherpa-onnx/cmake/show-info.cmake`](sherpa-rs-sys/sherpa-onnx/cmake/show-info.cmake)：不再调用新版 Windows 已移除的 WMIC，改用 CMake 提供的 host system 名称和版本。
- [`sherpa-rs-sys/sherpa-onnx/cmake/onnxruntime-win-x64.cmake`](sherpa-rs-sys/sherpa-onnx/cmake/onnxruntime-win-x64.cmake)：不再依赖仅 Visual Studio generator 提供的 `CMAKE_VS_PLATFORM_NAME`，改用目标处理器和指针宽度校验 Windows x64，使 Ninja 等 single-config generator 可用。

## `tauri-specta` 2.0.0-rc.21

**上游基线：** crates.io `tauri-specta` 2.0.0-rc.21。

**保留原因：** 上游发布包无条件启用 `tauri` 默认 features，会覆盖 Typex 在顶层选择的最小 feature 集，并让 Windows 测试进程在加载时引入 `TaskDialogIndirect`。本地元数据补丁保持 `tauri-specta` API 不变，同时避免该导入。

**Typex 修改：**

- [`tauri-specta/Cargo.toml`](tauri-specta/Cargo.toml)：为 `tauri` 依赖设置 `default-features = false`，仅保留显式的 `specta` feature。
- [`tauri-specta/src/lib.rs`](tauri-specta/src/lib.rs)：移除未使用的 `specta::Language` import；不改变 API 或运行时行为。
