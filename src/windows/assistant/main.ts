import { createApp } from "vue";
import { createPinia } from "pinia";
import "@/styles/base.css";
import { initTheme } from "@/composables/useTheme";
import { makeI18n } from "@/i18n";
import { syncLocale } from "@/composables/useLocale";
import Assistant from "./Assistant.vue";

const i18n = makeI18n();
createApp(Assistant).use(createPinia()).use(i18n).mount("#app");
syncLocale(i18n);
initTheme();
