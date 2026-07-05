// HUD 专用轻量 IPC（07 §11 HUD 极简纪律：不引 bindings.ts 全量、不引 Pinia）
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export type SessionMode = "dictation" | "translation" | "assistant";
export type SessionPhase =
  | "idle"
  | "recording"
  | "transcribing"
  | "processing"
  | "injecting"
  | "success"
  | "failed";
export type ErrorCode =
  | "auth_error"
  | "network_error"
  | "timeout"
  | "rate_limited"
  | "server_error"
  | "invalid_request"
  | "no_speech"
  | "no_focus"
  | "permission_missing"
  | "audio_device"
  | "not_configured"
  | "internal";

export interface SessionSnapshot {
  session_id: number;
  mode: SessionMode;
  phase: SessionPhase;
  recording_ms: number;
  verbatim: boolean;
  translation_direction: string | null;
  error: ErrorCode | null;
  failed_stage: string | null;
  has_transcript: boolean;
  unpolished: boolean;
  processing_step: string | null;
  busy_hint: boolean;
}

export function onSnapshot(cb: (snap: SessionSnapshot) => void) {
  return listen<SessionSnapshot>("session-snapshot-event", (e) => cb(e.payload));
}

export function onAudioLevel(cb: (levels: number[]) => void) {
  return listen<number[]>("audio-level-event", (e) => cb(e.payload));
}

export type SessionCommand =
  | "cancel"
  | "retry"
  | "dismiss"
  | "copy_transcript"
  | "inject_original";

export function sendCommand(command: SessionCommand) {
  return invoke("session_command", { command });
}

export function cycleTranslationTarget(): Promise<string> {
  return invoke("cycle_translation_target");
}

/// HUD 一键切原样模式（02 F-9）；返回切换后 verbatim 状态。
export function toggleVerbatim(): Promise<boolean> {
  return invoke("toggle_verbatim");
}

// ── 主题同步（04 §3.4：HUD 同样双主题，手动固定跟随设置）──

export type ThemeMode = "system" | "light" | "dark";

interface ThemeSlice {
  general: { theme: ThemeMode };
}

export function getThemeMode(): Promise<ThemeMode> {
  return invoke<ThemeSlice>("get_settings").then((s) => s.general.theme);
}

export function onThemeChanged(cb: (theme: ThemeMode) => void) {
  return listen<ThemeSlice>("settings-changed-event", (e) => cb(e.payload.general.theme));
}
