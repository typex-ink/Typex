import { createApp } from "vue";
import { createPinia } from "pinia";
import "@/styles/base.css";
import Onboarding from "./Onboarding.vue";

createApp(Onboarding).use(createPinia()).mount("#app");
