<p align="right">
  <strong>English</strong> | <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <img src="assets/icon/typex.svg" width="112" alt="Typex app icon" />
</p>

<h1 align="center">Typex</h1>

<p align="center">
  <strong>Say it, get it.</strong>
</p>

## About

Typex is an open-source, desktop AI voice input tool. Hold a shortcut and speak — Typex types the cleaned-up text directly into whatever app you're using.

## Features

- **Native-grade performance**: Built on Tauri 2 and Rust. Recording, global shortcuts, provider calls, local inference, and text injection all run on the Rust side, keeping Typex light and fast.
- **Speak anywhere**: Hold the global shortcut and talk naturally in any app — Typex inserts the polished text right at your cursor. It strips filler words, fixes punctuation, and tracks your intent even if you change your mind mid-sentence.
- **Speech translation**: Speak in one language, get text in another. Bidirectional translation makes multilingual workflows (like English/Chinese) effortless.
- **Context-aware voice assistant**: Select some text and dictate an instruction — rewrite, translate, clean up, summarize, or reformat it in place. If your instruction is a question, Typex shows the answer in a read-only popup instead.
- **BYOK by default**: No Typex account required. Bring your own STT and LLM services — Typex supports OpenAI-compatible APIs, OpenAI Responses, Volcano Engine/Doubao STT, Ollama or self-hosted endpoints, plus built-in presets for common providers.
- **Local and offline models**: Download models like SenseVoice, Qwen3-ASR, Whisper large-v3, or Qwen GGUF for fully offline STT and local LLM workflows.
- **Free and open source**: Typex is licensed under GPL-3.0. No mandatory account, no subscription wall.
- **Privacy-first by design**: Audio, selected text, and generated text are only ever sent to the endpoint you configure — with local models, nothing leaves your machine. Typex has no telemetry.

## Download

Get the latest release from the [GitHub Releases page](https://github.com/typex-ink/Typex/releases).

**On macOS**, if you see a warning that the app "cannot be verified to be free of malware" on launch, remove the quarantine attribute after installing:

```bash
sudo xattr -dr com.apple.quarantine /Applications/Typex.app
```

## Links

- Website: [typex.ink](https://typex.ink)

## License

Typex is licensed under GPL-3.0. See [LICENSE](LICENSE) for details.