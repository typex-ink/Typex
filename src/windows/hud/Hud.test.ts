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
  setHudSize: vi.fn(),
}));

vi.mock("./ipc", () => ({
  onSnapshot: ipc.onSnapshot,
  onAudioLevel: ipc.onAudioLevel,
  sendCommand: ipc.sendCommand,
  setHudSize: ipc.setHudSize,
  cycleTranslationTarget: vi.fn(async () => "English"),
  toggleVerbatim: vi.fn(async () => false),
}));

vi.mock("@tauri-apps/api/window", () => ({
  currentMonitor: vi.fn(async () => null),
}));

vi.mock("./Waveform.vue", () => ({
  default: { template: '<div class="wave" />' },
}));

let resizeObserverCallback: ResizeObserverCallback | null = null;
let hudWidth = 270;
let hudHeight = 44;

class ResizeObserverStub {
  constructor(callback: ResizeObserverCallback) {
    resizeObserverCallback = callback;
  }
  observe() {}
  disconnect() {}
}

function hudSnapshot(overrides: Partial<SessionSnapshot> = {}): SessionSnapshot {
  return {
    session_id: 7,
    mode: "dictation",
    phase: "recording",
    recording_ms: 0,
    verbatim: false,
    translation_direction: null,
    error: null,
    failed_stage: null,
    has_transcript: false,
    unpolished: false,
    processing_step: null,
    busy_hint: false,
    ...overrides,
  };
}

describe("Hud", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.clearAllMocks();
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    resizeObserverCallback = null;
    hudWidth = 270;
    hudHeight = 44;
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      () =>
        ({
          x: 0,
          y: 0,
          top: 0,
          right: hudWidth,
          bottom: hudHeight,
          left: 0,
          width: hudWidth,
          height: hudHeight,
          toJSON: () => ({}),
        }) as DOMRect,
    );
    ipc.snapshotHandler = null;
    ipc.setHudSize.mockResolvedValue(undefined);
    ipc.onSnapshot.mockImplementation(async (handler: (snapshot: SessionSnapshot) => void) => {
      ipc.snapshotHandler = handler;
      return () => {};
    });
    ipc.onAudioLevel.mockResolvedValue(() => {});
  });

  it("renders injection_blocked as copied information without retry actions", async () => {
    const wrapper = mount(Hud);
    await flushPromises();

    const snapshot = hudSnapshot({
      mode: "translation",
      phase: "failed",
      translation_direction: "Chinese -> English",
      error: "injection_blocked",
      failed_stage: "injecting",
      has_transcript: true,
    });
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

  it("serializes frame updates and applies only the latest observed size", async () => {
    let resolveFirstUpdate!: () => void;
    ipc.setHudSize
      .mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            resolveFirstUpdate = resolve;
          }),
      )
      .mockResolvedValue(undefined);

    const wrapper = mount(Hud);
    await flushPromises();
    ipc.snapshotHandler?.(hudSnapshot());
    await nextTick();
    await vi.waitFor(() => expect(ipc.setHudSize).toHaveBeenCalledTimes(1));
    expect(ipc.setHudSize).toHaveBeenNthCalledWith(1, 302, 76);

    hudWidth = 220;
    resizeObserverCallback?.([], {} as ResizeObserver);
    hudWidth = 180;
    resizeObserverCallback?.([], {} as ResizeObserver);
    expect(ipc.setHudSize).toHaveBeenCalledTimes(1);

    resolveFirstUpdate();
    await vi.waitFor(() => expect(ipc.setHudSize).toHaveBeenCalledTimes(2));
    expect(ipc.setHudSize).toHaveBeenNthCalledWith(2, 212, 76);

    wrapper.unmount();
  });
});
