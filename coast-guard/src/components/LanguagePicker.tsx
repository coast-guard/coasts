import { useState, useRef, useEffect } from 'react';
import { Globe } from '@phosphor-icons/react';
import { useLocale } from '../hooks/useLocale';
import { SUPPORTED_LANGUAGES, LANGUAGE_NAMES } from '../i18n';

export default function LanguagePicker() {
  const { locale, setLocale } = useLocale();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function handleClick(e: MouseEvent) {
      if (ref.current != null && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [open]);

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen((prev) => !prev)}
        className="h-8 inline-flex items-center gap-1.5 px-2 rounded-lg text-subtle-ui hover:bg-white/30 dark:hover:bg-white/10 transition-colors text-xs font-semibold uppercase"
        title={LANGUAGE_NAMES[locale]}
      >
        <Globe size={18} />
        <span>{locale}</span>
      </button>

      {open && (
        <div className="absolute right-0 top-full mt-2 glass-panel py-1 min-w-[160px] z-50">
          {SUPPORTED_LANGUAGES.map((lang) => (
            <button
              key={lang}
              onClick={() => {
                setLocale(lang);
                setOpen(false);
              }}
              className={`w-full text-left px-4 py-2 text-sm transition-colors ${
                lang === locale
                  ? 'text-main font-semibold bg-white/20 dark:bg-white/10'
                  : 'text-muted-ui hover:text-main hover:bg-white/15 dark:hover:bg-white/8'
              }`}
            >
              {LANGUAGE_NAMES[lang]}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
