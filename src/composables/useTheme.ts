// 主题同步（04 §3.4）：跟随系统 / 手动固定；所有窗口入口调用
import { commands, events, type ThemeMode } from "@/ipc/bindings";

function apply(theme: ThemeMode) {
  const root = document.documentElement;
  if (theme === "light" || theme === "dark") {
    root.setAttribute("data-theme", theme);
  } else {
    root.removeAttribute("data-theme"); // 跟随系统（tokens.css @media 生效）
  }
}

export async function initTheme() {
  try {
    const s = await commands.getSettings();
    apply(s.general.theme);
    await events.settingsChangedEvent.listen((e) => apply(e.payload.general.theme));
  } catch {
    // 非 Tauri 环境（测试）静默
  }
}
