import { useEffect, type ReactNode } from 'react';

interface ModalProps {
  readonly open: boolean;
  readonly title: string;
  readonly onClose: () => void;
  readonly children: ReactNode;
  readonly actions?: ReactNode | undefined;
  readonly wide?: boolean;
}

export default function Modal({ open, title, onClose, children, actions, wide }: ModalProps) {
  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose();
    }
    document.addEventListener('keydown', handleKey);
    document.body.style.overflow = 'hidden';
    return () => {
      document.removeEventListener('keydown', handleKey);
      document.body.style.overflow = '';
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-slate-950/45 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className={`glass-panel ${wide ? 'max-w-lg' : 'max-w-md'} w-full mx-4 border border-slate-200/80 dark:border-white/10`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-[var(--border)]">
          <h3 className="text-base font-semibold text-main">{title}</h3>
          <button
            onClick={onClose}
            className="h-7 w-7 flex items-center justify-center rounded-md text-subtle-ui hover:text-main hover:bg-white/30 dark:hover:bg-white/10 transition-colors"
          >
            ×
          </button>
        </div>
        <div className="px-5 py-4 text-sm text-main">
          {children}
        </div>
        {actions != null && (
          <div className="flex items-center justify-end gap-2 px-5 py-3 border-t border-[var(--border)]">
            {actions}
          </div>
        )}
      </div>
    </div>
  );
}
