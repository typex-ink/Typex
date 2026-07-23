import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { commands } from "@/ipc/bindings";
import DiagnosticsPage from "./DiagnosticsPage.vue";

const getDiagnostics = vi.hoisted(() => vi.fn());
const getPermissionStatus = vi.hoisted(() => vi.fn());
const openLogDir = vi.hoisted(() => vi.fn());
const openPermissionSettings = vi.hoisted(() => vi.fn());
const windowMocks = vi.hoisted(() => ({
  focus: null as null | ((event: { payload: boolean }) => void),
  unlistenFocus: vi.fn(),
  onFocusChanged: vi.fn(),
}));

enableAutoUnmount(afterEach);

vi.mock("@/ipc/bindings", () => ({
  commands: {
    getDiagnostics,
    getPermissionStatus,
    openLogDir,
    openPermissionSettings,
    exportDiagnostics: vi.fn(async () => ({ status: "ok", data: "C:\\diag.zip" })),
  },
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onFocusChanged: windowMocks.onFocusChanged,
  }),
}));

describe("DiagnosticsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    windowMocks.focus = null;
    windowMocks.onFocusChanged.mockImplementation(
      async (callback: (event: { payload: boolean }) => void) => {
        windowMocks.focus = callback;
        return windowMocks.unlistenFocus;
      },
    );
    getPermissionStatus.mockResolvedValue([]);
    getDiagnostics.mockResolvedValue({
      platform: "windows x86_64",
      permissions: [],
      platform_capabilities: [
        { key: "keyboard_hook", available: true, detail: "WH_KEYBOARD_LL healthy" },
        { key: "send_input", available: true, detail: "SendInput" },
        { key: "ui_automation", available: false, detail: "UIA unavailable" },
        { key: "webview2", available: true, detail: "Evergreen runtime active" },
        { key: "integrity", available: true, detail: "medium" },
      ],
      inject_backend: "SendInput Ctrl+V + Unicode SendInput",
      log_dir: "C:\\logs",
      hardware: "RAM 32 GB / Vulkan available",
    });
    openPermissionSettings.mockResolvedValue(undefined);
  });

  it("renders Windows capability health and details", async () => {
    const wrapper = mount(DiagnosticsPage, {
      global: { plugins: [makeI18n("en")] },
    });
    await flushPromises();

    expect(wrapper.text()).toContain("Platform: windows x86_64");
    expect(wrapper.text()).toContain("Low-level keyboard hook");
    expect(wrapper.text()).toContain("WH_KEYBOARD_LL healthy");
    expect(wrapper.text()).toContain("Windows text injection");
    expect(wrapper.text()).toContain("UI Automation selection");
    expect(wrapper.text()).toContain("UIA unavailable");
    expect(wrapper.text()).toContain("WebView2 Runtime");
    expect(wrapper.text()).toContain("medium");
    expect(wrapper.findAll(".bad")).toHaveLength(1);
  });

  it("opens the platform log directory", async () => {
    const wrapper = mount(DiagnosticsPage, {
      global: { plugins: [makeI18n("en")] },
    });
    await flushPromises();
    await wrapper.findAll("button")[0].trigger("click");

    expect(commands.openLogDir).toHaveBeenCalledOnce();
  });

  it("refreshes diagnostics after a permission request completes", async () => {
    getDiagnostics.mockResolvedValue({
      platform: "macos aarch64",
      permissions: [{ kind: "microphone", granted: false }],
      platform_capabilities: [],
      inject_backend: "CGEvent Cmd+V",
      log_dir: "/tmp/logs",
      hardware: null,
    });
    getPermissionStatus.mockResolvedValue([{ kind: "microphone", granted: true }]);

    const wrapper = mount(DiagnosticsPage, {
      global: { plugins: [makeI18n("en")] },
    });
    await flushPromises();

    const grant = wrapper.findAll("button").find((button) => button.text() === "Grant");
    expect(grant).toBeTruthy();
    await grant!.trigger("click");
    await flushPromises();

    expect(commands.openPermissionSettings).toHaveBeenCalledWith("microphone");
    expect(getDiagnostics).toHaveBeenCalledOnce();
    expect(getPermissionStatus).toHaveBeenCalledOnce();
    expect(wrapper.findAll("button").some((button) => button.text() === "Grant")).toBe(false);
    expect(wrapper.text()).toContain("Microphone permission");
  });

  it("refreshes permissions after returning from system settings", async () => {
    const denied = {
      platform: "macos aarch64",
      permissions: [{ kind: "microphone", granted: false }],
      platform_capabilities: [],
      inject_backend: "CGEvent Cmd+V",
      log_dir: "/tmp/logs",
      hardware: null,
    };
    getDiagnostics.mockResolvedValue(denied);
    getPermissionStatus
      .mockResolvedValueOnce([{ kind: "microphone", granted: false }])
      .mockResolvedValueOnce([{ kind: "microphone", granted: true }]);

    const wrapper = mount(DiagnosticsPage, {
      global: { plugins: [makeI18n("en")] },
    });
    await flushPromises();
    const grant = wrapper.findAll("button").find((button) => button.text() === "Grant");
    await grant!.trigger("click");
    await flushPromises();

    expect(wrapper.findAll("button").some((button) => button.text() === "Grant")).toBe(true);
    expect(getPermissionStatus).toHaveBeenCalledOnce();
    windowMocks.focus?.({ payload: false });
    await flushPromises();
    expect(getPermissionStatus).toHaveBeenCalledOnce();

    windowMocks.focus?.({ payload: true });
    await flushPromises();

    expect(getPermissionStatus).toHaveBeenCalledTimes(2);
    expect(wrapper.findAll("button").some((button) => button.text() === "Grant")).toBe(false);
    wrapper.unmount();
    expect(windowMocks.unlistenFocus).toHaveBeenCalledOnce();
  });
});
