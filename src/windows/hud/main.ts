// HUD 极简纪律（07 §11）：不引 Pinia / 路由 / Markdown，只有 StatusPill/Waveform/useSession
import { createApp } from "vue";
import "@/styles/base.css";
import Hud from "./Hud.vue";

// 主题：HUD 只跟随系统（毛玻璃 + token @media 已覆盖；手动固定主题经 settings 事件同步成本高，
// HUD 用途单一且半透明，跟随系统即可）
createApp(Hud).mount("#app");
