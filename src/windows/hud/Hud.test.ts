import { flushPromises, mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { beforeEach, describe, expect, it, vi } from "vitest";
import en from "@/i18n/en.json";
import Hud from "./Hud.vue";
import type { SessionSnapshot } from "./ipc";

const ipc = vi.hoisted(() => ({
  snapshotHandler: null as ((snapshot: SessionSnapshot) => void) | null,
  onSnapshot: vi.fn(),
  onAudioLevel: vi.fn(),
  sendCommand: vi.fn(),
}));

const windowApi = vi.hoisted(() => ({
  setSize: vi.fn(async () => {}),
  setPosition: vi.fn(async () => {}),
}));

vi.mock("./ipc", () => ({
  onSnapshot: ipc.onSnapshot,
  onAudioLevel: ipc.onAudioLevel,
  sendCommand: ipc.sendCommand,
  cycleTranslationTarget: vi.fn(async () => "English"),
  toggleVerbatim: vi.fn(async () => false),
}));

vi.mock("@tauri-apps/api/window", () => ({
  currentMonitor: vi.fn(async () => null),
  getCurrentWindow: vi.fn(() => windowApi),
}));

vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalPosition: class LogicalPosition {
    constructor(
      public x: number,
      public y: number,
    ) {}
  },
  LogicalSize: class LogicalSize {
    constructor(
      public width: number,
      public height: number,
    ) {}
  },
}));

class ResizeObserverStub {
  observe() {}
  disconnect() {}
}

describe("Hud", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    ipc.snapshotHandler = null;
    ipc.onSnapshot.mockImplementation(async (handler: (snapshot: SessionSnapshot) => void) => {
      ipc.snapshotHandler = handler;
      return () => {};
    });
    ipc.onAudioLevel.mockResolvedValue(() => {});
  });

  it("renders injection_blocked as copied information without retry actions", async () => {
    const wrapper = mount(Hud);
    await flushPromises();

    const snapshot: SessionSnapshot = {
      session_id: 7,
      mode: "translation",
      phase: "failed",
      recording_ms: 0,
      verbatim: false,
      translation_direction: "Chinese -> English",
      error: "injection_blocked",
      failed_stage: "injecting",
      has_transcript: true,
      unpolished: false,
      processing_step: null,
      busy_hint: false,
    };
    expect(ipc.snapshotHandler).not.toBeNull();
    ipc.snapshotHandler?.(snapshot);
    await nextTick();
    await flushPromises();

    expect(wrapper.find(".info").exists()).toBe(true);
    expect(wrapper.find(".warn").exists()).toBe(false);
    expect(wrapper.find(".ftext").text()).toBe(en.error.injection_blocked);
    expect(wrapper.find(".btn-sm").exists()).toBe(false);
    expect(wrapper.find(".btn-ghost-sm").exists()).toBe(false);

    wrapper.unmount();
  });
});
