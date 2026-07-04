import { createApp } from "vue";
import { createPinia } from "pinia";
import "@/styles/base.css";
import Home from "./Home.vue";

createApp(Home).use(createPinia()).mount("#app");
