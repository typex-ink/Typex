import { createApp } from "vue";
import { createPinia } from "pinia";
import "@/styles/base.css";
import { initTheme } from "@/composables/useTheme";

document.documentElement.classList.add("solid-window");
import Onboarding from "./Onboarding.vue";

createApp(Onboarding).use(createPinia()).mount("#app");
initTheme();
