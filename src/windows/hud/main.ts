// HUD 极简纪律（07 §11）：不引 Pinia / 路由 / Markdown，只有 StatusPill/Waveform/useSession
import { createApp } from "vue";
import "@/styles/base.css";
import Hud from "./Hud.vue";

createApp(Hud).mount("#app");
