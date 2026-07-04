import { createApp } from "vue";
import { createPinia } from "pinia";
import "@/styles/base.css";
import { initTheme } from "@/composables/useTheme";
import Home from "./Home.vue";

createApp(Home).use(createPinia()).mount("#app");
initTheme();
