import { computed, readonly, ref } from "vue";
import { platform as tauriPlatform } from "@tauri-apps/plugin-os";
import { canonicalKeyId } from "@/shared/hotkeys";

export type TypexPlatform = "macos" | "windows" | "linux" | "other";
type Translate = (key: string) => string;
type TranslationExists = (key: string) => boolean;

const PHYSICAL_KEY_LABELS: Readonly<Record<string, string>> = {
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
};

function physicalKeyLabel(key: string): string {
  const letter = /^Key([A-Z])$/.exec(key);
  if (letter) return letter[1];
  const digit = /^Digit([0-9])$/.exec(key);
  if (digit) return digit[1];
  const numpad = /^Numpad([0-9])$/.exec(key);
  if (numpad) return `Num ${numpad[1]}`;
  return PHYSICAL_KEY_LABELS[key] ?? key;
}

function detectPlatform(): TypexPlatform {
  try {
    const value = tauriPlatform();
    if (value === "macos" || value === "windows" || value === "linux") return value;
  } catch {
    // Component tests and browser previews run without the Tauri plugin.
  }
  const fallback = typeof navigator === "undefined" ? "" : navigator.userAgent.toLowerCase();
  if (fallback.includes("windows")) return "windows";
  if (fallback.includes("mac os") || fallback.includes("macintosh")) return "macos";
  if (fallback.includes("linux")) return "linux";
  return "other";
}

const current = ref<TypexPlatform>(detectPlatform());

export function usePlatform() {
  const isMacOS = computed(() => current.value === "macos");
  const isWindows = computed(() => current.value === "windows");
  const defaultHotkeys = computed(() =>
    isMacOS.value
      ? {
          dictation: ["MetaRight"],
          assistant: ["AltRight"],
          translation: ["MetaRight", "AltRight"],
        }
      : {
          dictation: ["ControlRight"],
          assistant: ["AltRight"],
          translation: ["ControlRight", "AltRight"],
        },
  );

  function keyLabel(key: string, t: Translate, te: TranslationExists): string {
    const canonical = canonicalKeyId(key);
    const platformKey = `${isMacOS.value ? "keys" : "keys_windows"}.${canonical}`;
    if (te(platformKey)) return t(platformKey);
    return te(`keys.${canonical}`) ? t(`keys.${canonical}`) : physicalKeyLabel(canonical);
  }

  function hotkeyConflictKey(key: string): string | null {
    const suffix = isMacOS.value ? "" : "_windows";
    const base: Record<string, string> = {
      CapsLock: `components.hotkey.conflict_capslock${suffix}`,
      MetaLeft: `components.hotkey.conflict_meta_left${suffix}`,
      AltLeft: `components.hotkey.conflict_alt${suffix}`,
    };
    return base[canonicalKeyId(key)] ?? null;
  }

  return {
    platform: readonly(current),
    isMacOS,
    isWindows,
    defaultHotkeys,
    keyLabel,
    hotkeyConflictKey,
  };
}
