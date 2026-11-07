import { createContext, useContext, useEffect, useMemo } from "react";
import type { ReactNode } from "react";
import { LOCALES, translator } from "./messages";
import {
  changePreferences,
  initPreferences,
  usePreferences,
} from "../preferences/store";
import type { Locale, MessageKey, Translate } from "./messages";

export { LOCALES };
export type { Locale, MessageKey, Translate };

interface I18n {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: Translate;
}

const I18nContext = createContext<I18n | null>(null);

const setLocale = (next: Locale) => {
    void changePreferences({ op: "locale", locale: next }).catch(
      () => undefined,
    );
  };

export function I18nProvider({ children }: { children: ReactNode }) {
  const locale = usePreferences((p) => p.saved.locale) ?? "en";
  const ready = usePreferences((p) => p.ready);
  useEffect(() => {
    void initPreferences();
  }, []);
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);


  const t = useMemo(() => translator(locale), [locale]);

  const value = useMemo(() => ({ locale, setLocale, t }), [locale, t]);
  return (
    <I18nContext.Provider value={value}>
      {ready ? children : null}
    </I18nContext.Provider>
  );
}

export function useI18n(): I18n {
  const value = useContext(I18nContext);
  if (!value) throw new Error("useI18n must be used inside I18nProvider");
  return value;
}

export function useT(): Translate {
  return useI18n().t;
}
