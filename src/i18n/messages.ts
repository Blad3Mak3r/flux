// The message catalogues and the locale the app starts in. No React, so state outside components
// can name things too.

import { en } from "./en.ts";
import type { MessageKey } from "./en.ts";
import { es } from "./es.ts";

export type { MessageKey };
export type Locale = "en" | "es";

export const LOCALES: { id: Locale; label: string }[] = [
  { id: "en", label: "English" },
  { id: "es", label: "Español" },
];

const MESSAGES: Record<Locale, Record<MessageKey, string>> = { en, es };
export const LOCALE_STORAGE_KEY = "flux.locale";

export type Translate = (key: MessageKey, values?: Record<string, string | number>) => string;

export function initialLocale(): Locale {
  try {
    const stored = localStorage.getItem(LOCALE_STORAGE_KEY);
    if (stored === "en" || stored === "es") return stored;
  } catch {
    // Storage can be unavailable; the default applies.
  }
  return "en";
}

export function translator(locale: Locale): Translate {
  return (key, values) => {
    const message = MESSAGES[locale][key];
    if (!values) return message;
    return message.replace(/\{(\w+)\}/g, (whole, name: string) =>
      name in values ? String(values[name]) : whole,
    );
  };
}
