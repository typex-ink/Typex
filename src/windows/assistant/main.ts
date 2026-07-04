import { createApp } from "vue";
import { createPinia } from "pinia";
import "@/styles/base.css";
import { initTheme } from "@/composables/useTheme";
import Assistant from "./Assistant.vue";

createApp(Assistant).use(createPinia()).mount("#app");
initTheme();
