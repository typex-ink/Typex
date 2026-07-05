import { createApp } from "vue";
import { createPinia } from "pinia";
import "@/styles/base.css";
import { initTheme } from "@/composables/useTheme";
import { makeI18n } from "@/i18n";
import { syncLocale } from "@/composables/useLocale";

document.documentElement.classList.add("solid-window", "chrome-window");
import Home from "./Home.vue";

const i18n = makeI18n();
createApp(Home).use(createPinia()).use(i18n).mount("#app");
syncLocale(i18n);
initTheme();
