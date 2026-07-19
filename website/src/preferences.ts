export type Locale = "en" | "zh-CN";
export type Theme = "light" | "dark";

export const LOCALE_STORAGE_KEY = "typex-site-locale";
export const THEME_STORAGE_KEY = "typex-site-theme";

export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function safeRead(storage: StorageLike | undefined, key: string): string | null {
  try {
    return storage?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

function safeWrite(storage: StorageLike | undefined, key: string, value: string): void {
  try {
    storage?.setItem(key, value);
  } catch {
    // Preferences remain usable for this visit when storage is unavailable.
  }
}

export function detectLocale(languages: readonly string[]): Locale {
  return (languages[0] ?? "en").toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

export function initialLocale(storage: StorageLike | undefined, languages: readonly string[]): Locale {
  const stored = safeRead(storage, LOCALE_STORAGE_KEY);
  return stored === "en" || stored === "zh-CN" ? stored : detectLocale(languages);
}

export function persistLocale(storage: StorageLike | undefined, locale: Locale): void {
  safeWrite(storage, LOCALE_STORAGE_KEY, locale);
}

export function initialTheme(storage: StorageLike | undefined, prefersDark: boolean): Theme {
  const stored = safeRead(storage, THEME_STORAGE_KEY);
  if (stored === "light" || stored === "dark") return stored;
  return prefersDark ? "dark" : "light";
}

export function hasStoredTheme(storage: StorageLike | undefined): boolean {
  const stored = safeRead(storage, THEME_STORAGE_KEY);
  return stored === "light" || stored === "dark";
}

export function persistTheme(storage: StorageLike | undefined, theme: Theme): void {
  safeWrite(storage, THEME_STORAGE_KEY, theme);
}
