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
      className="fixed inset-0 z-[100] flex items-center justify-center bg-[var(--overlay)] backdrop-blur-sm"
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div
        className={`glass-panel ${wide ? 'max-w-lg' : 'max-w-md'} w-full mx-4 border border-[var(--border-strong)] shadow-[var(--shadow-modal)] dark:shadow-[var(--shadow-modal-dark)]`}
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-[var(--border)]">
          <h3 className="text-base font-semibold text-main">{title}</h3>
          <button
            onClick={onClose}
            className="h-7 w-7 flex items-center justify-center rounded-md text-subtle-ui hover:text-main hover:bg-[var(--surface-hover)] transition-colors"
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
