import { useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { api } from '../api/endpoints';
import { SUPPORTED_LANGUAGES, type SupportedLanguage } from '../i18n';

function isSupported(lang: string): lang is SupportedLanguage {
  return SUPPORTED_LANGUAGES.includes(lang as SupportedLanguage);
}

export function useLocale() {
  const { i18n } = useTranslation();
  const i18nRef = useRef(i18n);
  i18nRef.current = i18n;

  // Fetch persisted language from the daemon; localStorage provides the fast initial load
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const { language } = await api.getLanguage();
        if (cancelled || !isSupported(language)) return;
        void i18nRef.current.changeLanguage(language);
        localStorage.setItem('coast-language', language);
      } catch {
        // localStorage / browser default (set during i18n init) is used as fallback
      }
    })();
    return () => { cancelled = true; };
  }, []);

  // Auto-update when language is changed from the CLI or another tab
  useEffect(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${protocol}//${window.location.host}/api/v1/events`;
    let ws: WebSocket;
    let reconnectTimer: ReturnType<typeof setTimeout>;

    function connect() {
      ws = new WebSocket(url);

      ws.addEventListener('message', (msg: MessageEvent<string>) => {
        try {
          const evt = JSON.parse(msg.data) as { event: string; language?: string };
          if (evt.event !== 'config.language_changed') return;
          const lang = evt.language;
          if (lang && isSupported(lang) && i18nRef.current.language !== lang) {
            void i18nRef.current.changeLanguage(lang);
            localStorage.setItem('coast-language', lang);
          }
        } catch { /* ignore malformed frames */ }
      });

      ws.addEventListener('close', () => {
        reconnectTimer = setTimeout(connect, 3000);
      });
      ws.addEventListener('error', () => ws.close());
    }

    connect();

    return () => {
      clearTimeout(reconnectTimer);
      if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CLOSING) {
        ws.close();
      } else {
        ws.addEventListener('open', () => ws.close());
      }
    };
  }, []);

  const setLocale = useCallback(
    (lang: SupportedLanguage) => {
      void i18n.changeLanguage(lang);
      localStorage.setItem('coast-language', lang);
      void api.setLanguage(lang);
    },
    [i18n],
  );

  return {
    locale: i18n.language as SupportedLanguage,
    setLocale,
  };
}
