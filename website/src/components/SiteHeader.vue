<script setup lang="ts">
import { Code2, Languages, Menu, Moon, Sun, X } from "@lucide/vue";
import { nextTick, onMounted, onUnmounted, ref } from "vue";
import type { SiteCopy } from "../content";
import type { Locale, Theme } from "../preferences";
import { SITE_LINKS } from "../links";
import logoUrl from "../../../assets/icon/typex.svg";

const props = defineProps<{
  copy: SiteCopy["nav"];
  locale: Locale;
  theme: Theme;
}>();

const emit = defineEmits<{
  toggleLocale: [];
  toggleTheme: [];
}>();

const menuOpen = ref(false);
const menuButton = ref<HTMLButtonElement>();

function closeMenu(restoreFocus = false): void {
  if (!menuOpen.value) return;
  menuOpen.value = false;
  if (restoreFocus) void nextTick(() => menuButton.value?.focus());
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape" && menuOpen.value) {
    event.preventDefault();
    closeMenu(true);
  }
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <header class="site-header">
    <div class="site-header__inner site-container">
      <a class="brand-link" href="#top" aria-label="Typex" @click="closeMenu()">
        <img :src="logoUrl" alt="" width="32" height="32" />
        <span>Typex</span>
      </a>

      <nav class="desktop-nav" :aria-label="copy.features">
        <a href="#features">{{ copy.features }}</a>
        <a href="#privacy">{{ copy.privacy }}</a>
        <a href="#download">{{ copy.download }}</a>
        <a :href="SITE_LINKS.repository" target="_blank" rel="noreferrer">
          <Code2 :size="17" aria-hidden="true" />
          {{ copy.github }}
        </a>
      </nav>

      <div class="header-actions">
        <button
          class="icon-control locale-control"
          type="button"
          :aria-label="copy.locale"
          :title="copy.locale"
          @click="emit('toggleLocale')"
        >
          <Languages :size="18" aria-hidden="true" />
          <span>{{ locale === "en" ? "中" : "EN" }}</span>
        </button>
        <button
          class="icon-control"
          type="button"
          :aria-label="theme === 'light' ? copy.themeDark : copy.themeLight"
          :title="theme === 'light' ? copy.themeDark : copy.themeLight"
          @click="emit('toggleTheme')"
        >
          <Moon v-if="theme === 'light'" :size="18" aria-hidden="true" />
          <Sun v-else :size="18" aria-hidden="true" />
        </button>
        <button
          ref="menuButton"
          class="icon-control menu-control"
          type="button"
          :aria-expanded="menuOpen"
          aria-controls="mobile-navigation"
          :aria-label="menuOpen ? copy.menuClose : copy.menuOpen"
          :title="menuOpen ? copy.menuClose : copy.menuOpen"
          @click="menuOpen = !menuOpen"
        >
          <X v-if="menuOpen" :size="20" aria-hidden="true" />
          <Menu v-else :size="20" aria-hidden="true" />
        </button>
      </div>
    </div>

    <nav
      v-if="menuOpen"
      id="mobile-navigation"
      class="mobile-nav"
      :aria-label="copy.features"
    >
      <a href="#features" @click="closeMenu()">{{ copy.features }}</a>
      <a href="#privacy" @click="closeMenu()">{{ copy.privacy }}</a>
      <a href="#download" @click="closeMenu()">{{ copy.download }}</a>
      <a :href="SITE_LINKS.repository" target="_blank" rel="noreferrer" @click="closeMenu()">
        <Code2 :size="18" aria-hidden="true" />
        {{ copy.github }}
      </a>
    </nav>
  </header>
</template>
