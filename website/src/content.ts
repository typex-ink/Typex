import type { Locale } from "./preferences";

export type FeatureKind = "dictation" | "translation" | "assistant" | "local";

export interface FeatureCopy {
  kind: FeatureKind;
  title: string;
  body: string;
  points: readonly [string, string];
  visual: {
    title: string;
    labelA: string;
    valueA: string;
    labelB: string;
    valueB: string;
    note: string;
  };
}

export interface SiteCopy {
  meta: {
    title: string;
    description: string;
  };
  nav: {
    features: string;
    privacy: string;
    download: string;
    github: string;
    menuOpen: string;
    menuClose: string;
    locale: string;
    themeLight: string;
    themeDark: string;
  };
  hero: {
    tagline: string;
    body: string;
    github: string;
    download: string;
    compatibility: string;
  };
  demo: {
    title: string;
    body: string;
    editorTitle: string;
    documentTitle: string;
    draft: string;
    result: string;
    listening: string;
    polishing: string;
    typed: string;
    mode: string;
    pause: string;
    play: string;
  };
  featuresIntro: {
    title: string;
    body: string;
  };
  features: readonly [FeatureCopy, FeatureCopy, FeatureCopy, FeatureCopy];
  openSource: {
    title: string;
    body: string;
    commitments: readonly [
      { title: string; body: string },
      { title: string; body: string },
      { title: string; body: string },
    ];
    source: string;
  };
  download: {
    title: string;
    body: string;
    releaseNote: string;
    platforms: readonly [
      { name: string; support: string; action: string; package: string },
      { name: string; support: string; action: string; package: string },
    ];
  };
  footer: {
    summary: string;
    license: string;
    source: string;
    rights: string;
  };
}

export const siteCopy: Record<Locale, SiteCopy> = {
  en: {
    meta: {
      title: "Typex - Speak. It types.",
      description:
        "Open-source desktop AI voice input for macOS and Windows. Dictate, translate, and edit in any app with your own or local models.",
    },
    nav: {
      features: "Features",
      privacy: "Privacy",
      download: "Download",
      github: "GitHub",
      menuOpen: "Open navigation",
      menuClose: "Close navigation",
      locale: "Switch to Simplified Chinese",
      themeLight: "Use light theme",
      themeDark: "Use dark theme",
    },
    hero: {
      tagline: "Speak. It types.",
      body:
        "Hold a shortcut, say what you mean, and Typex puts polished text wherever your cursor is. No app switching. No Typex account.",
      github: "View on GitHub",
      download: "Download Typex",
      compatibility: "For macOS 12+ and Windows 10/11 x64",
    },
    demo: {
      title: "From a rough thought to ready-to-send text.",
      body:
        "Typex listens while you speak, cleans up the detours, then types the final thought into the app already in front of you.",
      editorTitle: "Drafts - Project notes",
      documentTitle: "Release update",
      draft:
        "Let's, uh, move the release to Thursday - actually Friday afternoon - and tell the team the Windows build needs one more QA pass.",
      result:
        "Let's move the release to Friday afternoon and tell the team the Windows build needs one more QA pass.",
      listening: "Listening",
      polishing: "Polishing",
      typed: "Typed",
      mode: "Dictation",
      pause: "Pause demo",
      play: "Play demo",
    },
    featuresIntro: {
      title: "Voice input that stays out of your way.",
      body:
        "Typex lives in the tray and works at the cursor. Use the same quiet interaction for dictation, translation, selection edits, and private local workflows.",
    },
    features: [
      {
        kind: "dictation",
        title: "Speak naturally. Keep only what you meant.",
        body:
          "Talk into any app without rehearsing. Typex removes filler, repairs punctuation, and follows the correction when you change your mind mid-sentence.",
        points: [
          "Types directly at the active cursor",
          "Cleans filler words and mid-sentence corrections",
        ],
        visual: {
          title: "Message draft",
          labelA: "Heard",
          valueA: "Send it on Tuesday - no, Wednesday morning.",
          labelB: "Typed",
          valueB: "Send it on Wednesday morning.",
          note: "Correction resolved before insertion",
        },
      },
      {
        kind: "translation",
        title: "Say it once. Write it in another language.",
        body:
          "Move between languages without a copy-paste loop. Typex transcribes the intent, translates it, and inserts the result where you are working.",
        points: [
          "Works with multilingual speech and workflows",
          "Uses your selected STT and language models",
        ],
        visual: {
          title: "Reply composer",
          labelA: "Mandarin",
          valueA: "我们周五下午再确认最终版本。",
          labelB: "English",
          valueB: "Let's confirm the final version on Friday afternoon.",
          note: "ZH -> EN",
        },
      },
      {
        kind: "assistant",
        title: "Point at the text. Say what should change.",
        body:
          "Select text in the app you already use, then speak an instruction. Rewrite, summarize, translate, or ask a question without moving the content elsewhere.",
        points: [
          "Replaces selected text for editing requests",
          "Shows a read-only answer for questions",
        ],
        visual: {
          title: "Planning note",
          labelA: "Selected text",
          valueA: "The launch plan needs clearer owners and dates.",
          labelB: "Voice instruction",
          valueB: "Turn this into two concrete action items.",
          note: "Selection stays in context",
        },
      },
      {
        kind: "local",
        title: "Bring a key, or keep the whole workflow local.",
        body:
          "Connect OpenAI-compatible services, Ollama, or self-hosted endpoints. Download supported local speech and language models when the data should stay on your machine.",
        points: [
          "No mandatory Typex account or subscription",
          "No telemetry; local models keep content on-device",
        ],
        visual: {
          title: "Model routing",
          labelA: "Speech to text",
          valueA: "SenseVoice - Local",
          labelB: "Text processing",
          valueB: "Qwen GGUF - Local",
          note: "Offline workflow ready",
        },
      },
    ],
    openSource: {
      title: "Source you can inspect. Telemetry you never have to opt out of.",
      body:
        "Typex is built in public. Audio, selected text, and generated text go only to the endpoints you configure; with local models, they do not leave your machine.",
      commitments: [
        {
          title: "GPL-3.0",
          body: "Read the code, audit the behavior, and build on it under a strong copyleft license.",
        },
        {
          title: "Zero telemetry",
          body: "No analytics SDK, usage beacon, advertising identifier, or hidden product tracking.",
        },
        {
          title: "No Typex account",
          body: "Bring your own providers or use local models. The tool does not sit between you and them.",
        },
      ],
      source: "Inspect the source",
    },
    download: {
      title: "Say it once. Use it everywhere.",
      body:
        "Both buttons open the Typex GitHub Releases page. Choose the installer that matches your system there.",
      releaseNote: "You will choose the matching installation package on GitHub.",
      platforms: [
        {
          name: "macOS",
          support: "macOS 12 or later",
          action: "Choose macOS download",
          package: "Apple silicon and Intel release assets",
        },
        {
          name: "Windows",
          support: "Windows 10 22H2+ / Windows 11 x64",
          action: "Choose Windows download",
          package: "64-bit NSIS installer",
        },
      ],
    },
    footer: {
      summary: "Open-source desktop voice input.",
      license: "GPL-3.0 license",
      source: "Source code",
      rights: "Typex is free software licensed under GPL-3.0-only.",
    },
  },
  "zh-CN": {
    meta: {
      title: "Typex - 说，即所得。",
      description:
        "面向 macOS 与 Windows 的开源桌面 AI 语音输入工具。在任意应用听写、翻译和编辑，模型与密钥由你选择。",
    },
    nav: {
      features: "功能",
      privacy: "隐私",
      download: "下载",
      github: "GitHub",
      menuOpen: "打开导航",
      menuClose: "关闭导航",
      locale: "Switch to English",
      themeLight: "切换到亮色主题",
      themeDark: "切换到暗色主题",
    },
    hero: {
      tagline: "说，即所得。",
      body:
        "按住快捷键说话，Typex 会把整理好的文字直接放到光标所在的位置。不用切换应用，也不需要注册账号。",
      github: "在 GitHub 查看",
      download: "下载 Typex",
      compatibility: "支持 macOS 12+ 与 Windows 10/11 x64",
    },
    demo: {
      title: "从随口一说，到可以直接发送的文字。",
      body: "Typex 边听边整理，去掉犹豫和改口的部分，把最终表达写进你正在使用的应用。",
      editorTitle: "草稿 - 项目记录",
      documentTitle: "发布进度",
      draft: "我们，嗯，把发布改到周四——不对，周五下午，然后告诉团队 Windows 版本还要再测一轮。",
      result: "我们把发布改到周五下午，并告诉团队 Windows 版本还要再测一轮。",
      listening: "正在听",
      polishing: "正在整理",
      typed: "已输入",
      mode: "听写",
      pause: "暂停演示",
      play: "播放演示",
    },
    featuresIntro: {
      title: "安静待命，需要时才出现。",
      body:
        "Typex 常驻托盘，在光标处待命。同一种轻量交互，就能完成听写、翻译、选区处理，以及完全离线的私密工作流。",
    },
    features: [
      {
        kind: "dictation",
        title: "自然地说，Typex 只留下你真正想表达的内容。",
        body:
          "不用先在脑子里打好草稿。Typex 会去掉语气词、修正标点，并在你说到一半改口时，准确捕捉你的最终意图。",
        points: ["直接写入当前应用的光标位置", "自动清理语气词与中途改口"],
        visual: {
          title: "消息草稿",
          labelA: "听到",
          valueA: "周二发出去，不对，周三上午。",
          labelB: "输入",
          valueB: "周三上午发出去。",
          note: "改口在写入前已处理",
        },
      },
      {
        kind: "translation",
        title: "只说一次，直接写成另一种语言。",
        body:
          "不用再来回复制粘贴。Typex 先识别你的意图，完成翻译后，直接把结果插入你正在编辑的位置。",
        points: ["适用于多语言口述与跨语言协作", "使用你选择的语音和语言模型"],
        visual: {
          title: "回复编辑器",
          labelA: "中文",
          valueA: "我们周五下午再确认最终版本。",
          labelB: "English",
          valueB: "Let's confirm the final version on Friday afternoon.",
          note: "中 -> EN",
        },
      },
      {
        kind: "assistant",
        title: "选中文字，说出你想怎么改。",
        body:
          "在当前应用里选中内容，直接说出指令。改写、总结、翻译或提问，都不用把文字搬到另一个窗口。",
        points: ["编辑型指令直接替换选中文字", "问答型指令在只读窗口显示结果"],
        visual: {
          title: "规划记录",
          labelA: "选中文字",
          valueA: "发布计划需要更明确的负责人和日期。",
          labelB: "语音指令",
          valueB: "把它改成两条具体行动项。",
          note: "选区上下文始终保留",
        },
      },
      {
        kind: "local",
        title: "自带密钥，或让整个流程留在本机。",
        body:
          "接入 OpenAI 兼容服务、Ollama 或自建端点；也可以下载受支持的本地语音与语言模型，让数据完全留在设备上。",
        points: ["不强制注册 Typex 账号或订阅", "无遥测收集，本地模型下内容不出设备"],
        visual: {
          title: "模型路由",
          labelA: "语音转文字",
          valueA: "SenseVoice - 本地",
          labelB: "文本处理",
          valueB: "Qwen GGUF - 本地",
          note: "离线工作流已就绪",
        },
      },
    ],
    openSource: {
      title: "源码公开可查，也没有遥测需要你关闭。",
      body:
        "Typex 在公开仓库中开发。音频、选中文字和生成结果只会发往你配置的端点；使用本地模型时，它们不会离开你的设备。",
      commitments: [
        {
          title: "GPL-3.0",
          body: "阅读源码、审计行为，并在强 copyleft 许可证下继续构建。",
        },
        {
          title: "零遥测",
          body: "没有分析 SDK、使用信标、广告标识，也没有隐藏的产品追踪。",
        },
        {
          title: "无需 Typex 账号",
          body: "使用自己的服务商或本地模型，Typex 不横在你和模型之间。",
        },
      ],
      source: "检查源代码",
    },
    download: {
      title: "说一次，处处可用。",
      body: "两个按钮都会打开 Typex 的 GitHub Releases 页面，请在那里选择与你系统匹配的安装包。",
      releaseNote: "你将在 GitHub 上选择对应平台的安装包。",
      platforms: [
        {
          name: "macOS",
          support: "macOS 12 及以上",
          action: "选择 macOS 下载",
          package: "Apple 芯片与 Intel 版本资产",
        },
        {
          name: "Windows",
          support: "Windows 10 22H2+ / Windows 11 x64",
          action: "选择 Windows 下载",
          package: "64 位 NSIS 安装器",
        },
      ],
    },
    footer: {
      summary: "开源桌面语音输入。",
      license: "GPL-3.0 许可证",
      source: "源代码",
      rights: "Typex 是基于 GPL-3.0-only 许可的自由软件。",
    },
  },
};
