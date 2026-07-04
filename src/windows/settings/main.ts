import { createApp } from "vue";
import { createPinia } from "pinia";
import "@/styles/base.css";
import { initTheme } from "@/composables/useTheme";
import Settings from "./Settings.vue";

createApp(Settings).use(createPinia()).mount("#app");
initTheme();
