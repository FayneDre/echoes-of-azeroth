// Minimal, dependency-free i18n. English is the source of truth: every key
// must exist in en.json; other locales fall back to English per key.

import en from "./locales/en.json";
import fr from "./locales/fr.json";

export type Locale = "en" | "fr";
export type MessageKey = keyof typeof en;
export type LocalizedText = { en: string; fr?: string };

export const LOCALES: { id: Locale; label: string }[] = [
  { id: "fr", label: "Français" },
  { id: "en", label: "English" },
];

const dictionaries: Record<Locale, Partial<Record<MessageKey, string>>> = { en, fr };
const STORAGE_KEY = "eoa.locale";

function initialLocale(): Locale {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "en" || saved === "fr") return saved;
  } catch {
    // Storage unavailable: fall through to the system language.
  }
  return navigator.language.toLowerCase().startsWith("fr") ? "fr" : "en";
}

let current = $state<Locale>(initialLocale());

export const i18n = {
  get locale(): Locale {
    return current;
  },
  set locale(value: Locale) {
    current = value;
    document.documentElement.lang = value;
    try {
      localStorage.setItem(STORAGE_KEY, value);
    } catch {
      // Not persisted; the choice still applies to this session.
    }
  },
};

/** Translates a UI message. `{name}` placeholders are replaced from `params`. */
export function t(key: MessageKey, params: Record<string, string | number> = {}): string {
  const template = dictionaries[current][key] ?? en[key];
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

/** Picks the current locale from catalog text, falling back to English. */
export function localized(text: LocalizedText): string {
  return (current === "fr" ? text.fr : undefined) ?? text.en;
}
