// HUD 极简纪律（06 §11）：不引 Pinia / 路由 / Markdown，只有 StatusPill/Waveform/useSession
import { createApp } from "vue";
import "@/styles/base.css";
import Hud from "./Hud.vue";
import { getThemeMode, onThemeChanged, type ThemeMode } from "./ipc";

// 主题（04 §3.4）：HUD 同样跟随「设置 → 通用 → 主题」；system = 移除属性走 tokens.css @media
function applyTheme(theme: ThemeMode) {
  const root = document.documentElement;
  if (theme === "light" || theme === "dark") {
    root.setAttribute("data-theme", theme);
  } else {
    root.removeAttribute("data-theme");
  }
}
getThemeMode().then(applyTheme).catch(() => {});
onThemeChanged(applyTheme);

createApp(Hud).mount("#app");
