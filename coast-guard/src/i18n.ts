import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';

import en from './locales/en.json';
import zh from './locales/zh.json';
import ja from './locales/ja.json';
import ko from './locales/ko.json';
import ru from './locales/ru.json';
import pt from './locales/pt.json';
import es from './locales/es.json';

export const SUPPORTED_LANGUAGES = ['en', 'zh', 'ja', 'ko', 'ru', 'pt', 'es'] as const;
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

export const LANGUAGE_NAMES: Readonly<Record<SupportedLanguage, string>> = {
  en: 'English',
  zh: '中文',
  ja: '日本語',
  ko: '한국어',
  ru: 'Русский',
  pt: 'Português',
  es: 'Español',
};

// Synchronous fast-path for i18next initialization.
//
// The daemon (coastd) is the source of truth for the user's language
// preference — shared between the CLI and this web UI.  Because i18n.init()
// is synchronous, we cannot await the daemon API here.  Instead we read
// from localStorage, which the useLocale hook keeps in sync with the daemon:
//
//   1. On mount, useLocale fetches GET /api/v1/config/language and writes
//      the result to both i18next and localStorage.
//   2. A WebSocket listener on the "config.language_changed" event does the
//      same, so changes made from the CLI propagate instantly.
//
// This means the very first render may briefly show a stale locale if the
// CLI changed the language while the browser was closed — useLocale corrects
// it within milliseconds of mount.
function getInitialLanguage(): SupportedLanguage {
  const stored = localStorage.getItem('coast-language');
  if (stored != null && SUPPORTED_LANGUAGES.includes(stored as SupportedLanguage)) {
    return stored as SupportedLanguage;
  }
  const browserLang = navigator.language.slice(0, 2);
  if (SUPPORTED_LANGUAGES.includes(browserLang as SupportedLanguage)) {
    return browserLang as SupportedLanguage;
  }
  return 'en';
}

void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    zh: { translation: zh },
    ja: { translation: ja },
    ko: { translation: ko },
    ru: { translation: ru },
    pt: { translation: pt },
    es: { translation: es },
  },
  lng: getInitialLanguage(),
  fallbackLng: 'en',
  interpolation: {
    escapeValue: false,
  },
});

export default i18n;
