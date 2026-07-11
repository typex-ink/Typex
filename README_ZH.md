<p align="right">
  <a href="README.md">English</a> | <strong>简体中文</strong>
</p>

<p align="center">
  <img src="assets/icon/typex.svg" width="112" alt="Typex 应用图标" />
</p>

<h1 align="center">Typex</h1>

<p align="center">
  <strong>说，即所得</strong>
</p>

## 这是什么

Typex 是一款开源的桌面 AI 语音输入工具。按住快捷键说话，松开后，整理干净的文字就会出现在你正在使用的任何应用里——不用切换窗口，不用手动整理。

## 功能亮点

- **原生级性能**：基于 Tauri 2 + Rust 构建，录音、全局快捷键、模型调用、本地推理和文本注入全部在 Rust 侧完成，启动快、占用低。
- **随处可说**：在任意应用中按住全局快捷键说话，文字会直接插入光标处。Typex 会自动去掉“嗯啊呃”之类的语气词、修正标点，即使你说到一半改口，也能准确捕捉你最终想表达的意思。
- **即说即译**：说一种语言，输出另一种语言，支持双向互译，中英混合办公也能顺畅衔接。
- **懂上下文的语音助手**：选中一段文字后直接口述指令,可以改写、翻译、润色、总结或重新排版；如果你问的是问题,则会弹出一个只读的回答窗口。
- **BYOK，自己的密钥自己配**：不绑定任何 Typex 账号。你可以自由接入 OpenAI 兼容接口、OpenAI Responses、火山引擎/豆包 STT、Ollama 或自建端点,内置常用服务的快捷预设。
- **本地模型,离线也能用**：支持下载 SenseVoice、Qwen3-ASR、Whisper large-v3、Qwen GGUF 等模型,离线语音识别和本地大模型工作流全都覆盖。
- **完全开源,零门槛**：GPL-3.0 协议开源,没有强制账号,没有订阅墙。
- **隐私优先**：音频、选中文本和生成结果只会发往你自己配置的服务端点;使用本地模型时,数据完全不出本机。Typex 不收集任何遥测数据。

## 下载安装

前往 [GitHub Releases 页面](https://github.com/typex-ink/Typex/releases) 下载最新版本。

Windows 版本支持 64 位 Windows 10 22H2 及以上版本和 Windows 11。NSIS 安装器使用 WebView2 Evergreen Bootstrapper；如果系统尚未安装 WebView2，安装时需要联网。本地 AI 原生运行库、所需 VC++ runtime 与 Vulkan loader 均随应用目录分发，无需另装开发工具或系统级 VC++/Vulkan runtime。

当前 Windows 候选构建尚未进行 Authenticode 发布者签名，因此 Windows 可能显示“未知发布者”或 Microsoft Defender SmartScreen 提示。选择“更多信息 > 仍要运行”前，请先核对随版本提供的 SHA-256 校验值和 Tauri updater 签名。启用 WDAC、AppLocker 或 Smart App Control 的受管设备可能会直接阻止未签名构建。

**macOS 用户注意**:如果启动时提示"无法验证是否包含恶意软件",安装后运行以下命令移除隔离属性即可:

```bash
sudo xattr -dr com.apple.quarantine /Applications/Typex.app
```

## 相关链接

- 官网：[typex.ink](https://typex.ink)

## 许可证

Typex 基于 GPL-3.0 许可证开源,详见 [LICENSE](LICENSE)。
