import { useState, useRef, useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { Palette } from '@phosphor-icons/react';
import type { TerminalThemeDef } from '../hooks/useTerminalTheme';

interface Props {
  readonly themes: readonly TerminalThemeDef[];
  readonly activeId: string;
  readonly onSelect: (id: string) => void;
}

export default function TerminalThemePicker({ themes, activeId, onSelect }: Props) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ top: 0, right: 0 });

  const updatePosition = useCallback(() => {
    if (btnRef.current == null) return;
    const rect = btnRef.current.getBoundingClientRect();
    setPos({
      top: rect.bottom + 6,
      right: window.innerWidth - rect.right,
    });
  }, []);

  useEffect(() => {
    if (!open) return;
    updatePosition();
    function handleClick(e: MouseEvent) {
      if (
        menuRef.current != null && !menuRef.current.contains(e.target as Node) &&
        btnRef.current != null && !btnRef.current.contains(e.target as Node)
      ) {
        setOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [open, updatePosition]);

  return (
    <>
      <button
        ref={btnRef}
        onClick={() => { updatePosition(); setOpen((prev) => !prev); }}
        className="h-8 w-8 inline-flex items-center justify-center rounded-lg text-subtle-ui hover:text-main hover:bg-white/25 dark:hover:bg-white/10 transition-colors shrink-0"
        title={t('termTheme.label')}
      >
        <Palette size={18} />
      </button>

      {open && createPortal(
        <div
          ref={menuRef}
          className="fixed glass-panel py-1 min-w-[180px] z-[300]"
          style={{ top: pos.top, right: pos.right }}
        >
          {themes.map((theme) => (
            <button
              key={theme.id}
              onClick={() => {
                onSelect(theme.id);
                setOpen(false);
              }}
              className={`w-full text-left px-3 py-2 text-sm flex items-center gap-3 transition-colors ${
                theme.id === activeId
                  ? 'text-main font-semibold bg-white/20 dark:bg-white/10'
                  : 'text-muted-ui hover:text-main hover:bg-white/15 dark:hover:bg-white/8'
              }`}
            >
              <span
                className="h-4 w-4 rounded-full shrink-0 border border-[var(--border)]"
                style={{ background: theme.colors.background }}
              />
              <span>{t(theme.labelKey)}</span>
            </button>
          ))}
        </div>,
        document.body,
      )}
    </>
  );
}
