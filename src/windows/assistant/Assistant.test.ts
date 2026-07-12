import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import Assistant from "./Assistant.vue";

enableAutoUnmount(afterEach);

const mocks = vi.hoisted(() => ({
  started: null as null | ((event: { payload: any }) => void),
  delta: null as null | ((event: { payload: any }) => void),
  done: null as null | ((event: { payload: any }) => void),
  error: null as null | ((event: { payload: any }) => void),
  focus: null as null | ((event: { payload: boolean }) => void),
  sessionCommand: vi.fn(async () => undefined),
  ready: vi.fn(async () => undefined),
  hide: vi.fn(async () => undefined),
  setSize: vi.fn(async () => undefined),
  setPosition: vi.fn(async () => undefined),
}));

function listener(name: "started" | "delta" | "done" | "error") {
  return {
    listen: vi.fn(async (callback: (event: { payload: any }) => void) => {
      mocks[name] = callback;
      return vi.fn();
    }),
  };
}

vi.mock("@/ipc/bindings", () => ({
  commands: {
    assistantWindowReady: mocks.ready,
    sessionCommand: mocks.sessionCommand,
  },
  events: {
    assistantStartedEvent: listener("started"),
    assistantDeltaEvent: listener("delta"),
    assistantDoneEvent: listener("done"),
    assistantErrorEvent: listener("error"),
  },
}));

vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalPosition: class LogicalPosition {
    constructor(public x: number, public y: number) {}
  },
  LogicalSize: class LogicalSize {
    constructor(public width: number, public height: number) {}
  },
}));

vi.mock("@tauri-apps/api/window", () => ({
  currentMonitor: vi.fn(async () => null),
  getCurrentWindow: () => ({
    hide: mocks.hide,
    setSize: mocks.setSize,
    setPosition: mocks.setPosition,
    scaleFactor: vi.fn(async () => 1),
    outerPosition: vi.fn(async () => ({ toLogical: () => ({ x: 0, y: 0 }) })),
    onFocusChanged: vi.fn(async (callback: (event: { payload: boolean }) => void) => {
      mocks.focus = callback;
      return vi.fn();
    }),
  }),
}));

class TestResizeObserver {
  observe() {}
  disconnect() {}
}

async function mountAssistant() {
  const wrapper = mount(Assistant, {
    global: { plugins: [makeI18n("en")] },
  });
  await flushPromises();
  return wrapper;
}

function start(requestId = 1) {
  mocks.started?.({
    payload: {
      request_id: requestId,
      instruction: "Explain the failure",
      selection_chars: 12,
      degraded: false,
    },
  });
}

describe("Assistant window", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    mocks.started = null;
    mocks.delta = null;
    mocks.done = null;
    mocks.error = null;
    mocks.focus = null;
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("resets, streams, completes, and escapes raw HTML", async () => {
    const wrapper = await mountAssistant();
    start();
    mocks.delta?.({
      payload: { request_id: 1, text_delta: "<script>bad()</script> **safe**" },
    });
    vi.advanceTimersByTime(34);
    await flushPromises();

    expect(wrapper.text()).toContain("Explain the failure");
    expect(wrapper.find("script").exists()).toBe(false);
    expect(wrapper.find("strong").text()).toBe("safe");

    mocks.done?.({ payload: { request_id: 1, full_text: "Final answer" } });
    await flushPromises();
    expect(wrapper.text()).toContain("Final answer");
    expect(wrapper.text()).not.toContain("Waiting for model");
  });

  it("cancels an active stream from Escape and close", async () => {
    const wrapper = await mountAssistant();
    start();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await flushPromises();

    expect(mocks.sessionCommand).toHaveBeenCalledWith("cancel");
    expect(mocks.hide).toHaveBeenCalledOnce();

    start(2);
    await wrapper.get(".x").trigger("click");
    await flushPromises();
    expect(mocks.sessionCommand).toHaveBeenCalledTimes(2);
  });

  it("cancels on focus loss but not after a completed answer", async () => {
    const wrapper = await mountAssistant();
    start();
    mocks.focus?.({ payload: false });
    await flushPromises();
    expect(mocks.sessionCommand).toHaveBeenCalledOnce();

    start(2);
    mocks.done?.({ payload: { request_id: 2, full_text: "Done" } });
    await flushPromises();
    mocks.sessionCommand.mockClear();
    await wrapper.get(".x").trigger("click");
    await flushPromises();
    expect(mocks.sessionCommand).not.toHaveBeenCalled();
    expect(mocks.hide).toHaveBeenCalled();
  });
});
