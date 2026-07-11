import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { makeI18n } from "@/i18n";
import { commands } from "@/ipc/bindings";
import DiagnosticsPage from "./DiagnosticsPage.vue";

const getDiagnostics = vi.hoisted(() => vi.fn());
const openLogDir = vi.hoisted(() => vi.fn());

vi.mock("@/ipc/bindings", () => ({
  commands: {
    getDiagnostics,
    openLogDir,
    openPermissionSettings: vi.fn(),
    exportDiagnostics: vi.fn(async () => ({ status: "ok", data: "C:\\diag.zip" })),
  },
}));

describe("DiagnosticsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
});
