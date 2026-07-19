<script setup lang="ts">
import {
  Apple,
  ArrowDownToLine,
  ArrowRight,
  Code2,
  ExternalLink,
  Monitor,
  UserRoundX,
  WifiOff,
} from "@lucide/vue";
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import FeatureSection from "./components/FeatureSection.vue";
import ProductDemo from "./components/ProductDemo.vue";
import SiteHeader from "./components/SiteHeader.vue";
import { siteCopy } from "./content";
import { SITE_LINKS } from "./links";
import {
  hasStoredTheme,
  initialLocale,
  initialTheme,
  persistLocale,
  persistTheme,
  type Locale,
  type StorageLike,
  type Theme,
} from "./preferences";

function browserStorage(): StorageLike | undefined {
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

const storage = browserStorage();
const darkModeQuery = window.matchMedia("(prefers-color-scheme: dark)");
const browserLanguages = navigator.languages.length ? navigator.languages : [navigator.language];
const locale = ref<Locale>(initialLocale(storage, browserLanguages));
const theme = ref<Theme>(initialTheme(storage, darkModeQuery.matches));
const themeIsExplicit = ref(hasStoredTheme(storage));
const copy = computed(() => siteCopy[locale.value]);

const commitmentIcons = [Code2, WifiOff, UserRoundX] as const;

function toggleLocale(): void {
  locale.value = locale.value === "en" ? "zh-CN" : "en";
  persistLocale(storage, locale.value);
}

function toggleTheme(): void {
  theme.value = theme.value === "light" ? "dark" : "light";
  themeIsExplicit.value = true;
  persistTheme(storage, theme.value);
  document.documentElement.dataset.theme = theme.value;
}

function onSystemThemeChange(event: MediaQueryListEvent): void {
  if (themeIsExplicit.value) return;
  theme.value = event.matches ? "dark" : "light";
  document.documentElement.removeAttribute("data-theme");
}

watch(
  copy,
  (nextCopy) => {
    document.documentElement.lang = locale.value;
    document.title = nextCopy.meta.title;
    document.querySelector<HTMLMetaElement>('meta[name="description"]')?.setAttribute(
      "content",
      nextCopy.meta.description,
    );
    document.querySelector<HTMLMetaElement>('meta[property="og:title"]')?.setAttribute(
      "content",
      nextCopy.meta.title,
    );
    document.querySelector<HTMLMetaElement>('meta[property="og:description"]')?.setAttribute(
      "content",
      nextCopy.meta.description,
    );
    document.querySelector<HTMLMetaElement>('meta[property="og:image:alt"]')?.setAttribute(
      "content",
      nextCopy.meta.title,
    );
  },
  { immediate: true },
);

onMounted(() => darkModeQuery.addEventListener("change", onSystemThemeChange));
onUnmounted(() => darkModeQuery.removeEventListener("change", onSystemThemeChange));
</script>

<template>
  <SiteHeader
    :copy="copy.nav"
    :locale="locale"
    :theme="theme"
    @toggle-locale="toggleLocale"
    @toggle-theme="toggleTheme"
  />

  <main id="top">
    <section class="hero site-container" aria-labelledby="hero-title">
      <div class="hero__content">
        <div class="hero__brand-lockup">
          <h1 id="hero-title">Typex</h1>
          <p class="hero__tagline">{{ copy.hero.tagline }}</p>
        </div>
        <p class="hero__body">{{ copy.hero.body }}</p>
        <div class="hero__actions">
          <a class="button button--primary" :href="SITE_LINKS.repository" target="_blank" rel="noreferrer">
            <Code2 :size="19" aria-hidden="true" />
            {{ copy.hero.github }}
          </a>
          <a class="button button--secondary" href="#download">
            <ArrowDownToLine :size="19" aria-hidden="true" />
            {{ copy.hero.download }}
          </a>
        </div>
        <p class="hero__compatibility">{{ copy.hero.compatibility }}</p>
      </div>
    </section>

    <section class="demo-band" aria-labelledby="demo-title">
      <div class="site-container site-container--wide">
        <div class="section-intro section-intro--demo">
          <h2 id="demo-title">{{ copy.demo.title }}</h2>
          <p>{{ copy.demo.body }}</p>
        </div>
        <ProductDemo :copy="copy.demo" />
      </div>
    </section>

    <section id="features" class="features-section" aria-labelledby="features-title">
      <div class="site-container">
        <div class="section-intro">
          <h2 id="features-title">{{ copy.featuresIntro.title }}</h2>
          <p>{{ copy.featuresIntro.body }}</p>
        </div>
        <div class="feature-list">
          <FeatureSection
            v-for="(feature, index) in copy.features"
            :key="feature.kind"
            :feature="feature"
            :reverse="index % 2 === 1"
          />
        </div>
      </div>
    </section>

    <section id="privacy" class="open-band" aria-labelledby="privacy-title">
      <div class="site-container">
        <div class="open-band__header">
          <h2 id="privacy-title">{{ copy.openSource.title }}</h2>
          <p>{{ copy.openSource.body }}</p>
          <a class="text-link" :href="SITE_LINKS.repository" target="_blank" rel="noreferrer">
            {{ copy.openSource.source }}
            <ArrowRight :size="17" aria-hidden="true" />
          </a>
        </div>
        <div class="commitments">
          <article v-for="(item, index) in copy.openSource.commitments" :key="item.title">
            <component :is="commitmentIcons[index]" :size="22" aria-hidden="true" />
            <h3>{{ item.title }}</h3>
            <p>{{ item.body }}</p>
          </article>
        </div>
      </div>
    </section>

    <section id="download" class="download-section" aria-labelledby="download-title">
      <div class="site-container">
        <div class="section-intro section-intro--center">
          <h2 id="download-title">{{ copy.download.title }}</h2>
          <p>{{ copy.download.body }}</p>
        </div>
        <div class="download-grid">
          <article v-for="(platform, index) in copy.download.platforms" :key="platform.name">
            <Apple v-if="index === 0" :size="28" aria-hidden="true" />
            <Monitor v-else :size="28" aria-hidden="true" />
            <div class="download-card__copy">
              <h3>{{ platform.name }}</h3>
              <strong>{{ platform.support }}</strong>
              <p>{{ platform.package }}</p>
            </div>
            <a
              class="button button--primary"
              :href="SITE_LINKS.releases"
              target="_blank"
              rel="noreferrer"
            >
              {{ platform.action }}
              <ExternalLink :size="17" aria-hidden="true" />
            </a>
          </article>
        </div>
        <p class="release-note">{{ copy.download.releaseNote }}</p>
      </div>
    </section>
  </main>

  <footer class="site-footer">
    <div class="site-container site-footer__inner">
      <div><strong>Typex</strong><span>{{ copy.footer.summary }}</span></div>
      <nav aria-label="Typex">
        <a :href="SITE_LINKS.license" target="_blank" rel="noreferrer">{{ copy.footer.license }}</a>
        <a :href="SITE_LINKS.repository" target="_blank" rel="noreferrer">{{ copy.footer.source }}</a>
      </nav>
      <p>{{ copy.footer.rights }}</p>
    </div>
  </footer>
</template>
