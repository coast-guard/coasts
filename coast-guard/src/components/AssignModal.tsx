import { useState, useMemo, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import Modal from './Modal';

interface AssignModalProps {
  readonly open: boolean;
  readonly instanceName: string;
  readonly worktrees: readonly string[];
  readonly occupiedWorktrees: ReadonlySet<string>;
  readonly onAssign: (worktree: string) => void;
  readonly onClose: () => void;
}

const inputClass =
  'w-full h-9 px-3 text-sm rounded-md border border-slate-300 dark:border-[var(--border)] bg-white/70 dark:bg-transparent text-main outline-none focus:border-[var(--primary)] placeholder:text-slate-500 dark:placeholder:text-subtle-ui';

export default function AssignModal({
  open, instanceName, worktrees, occupiedWorktrees, onAssign, onClose,
}: AssignModalProps) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<string | null>(null);
  const [customInput, setCustomInput] = useState('');
  const [filter, setFilter] = useState('');

  useEffect(() => {
    if (!open) {
      setSelected(null);
      setCustomInput('');
      setFilter('');
    }
  }, [open]);

  const availableWorktrees = useMemo(() => {
    const lowerFilter = filter.toLowerCase();
    return worktrees
      .filter((w) => !lowerFilter || w.toLowerCase().includes(lowerFilter))
      .map((w) => ({ name: w, occupied: occupiedWorktrees.has(w) }));
  }, [worktrees, occupiedWorktrees, filter]);

  const hasExistingWorktrees = worktrees.length > 0;
  const resolvedWorktree = customInput.trim() || selected;

  return (
    <Modal
      open={open}
      title={`${t('assign.modalTitle')} \u2014 ${instanceName}`}
      onClose={onClose}
      actions={
        <>
          <button onClick={onClose} className="btn btn-outline">
            {t('action.cancel')}
          </button>
          <button
            disabled={!resolvedWorktree}
            onClick={() => { if (resolvedWorktree) onAssign(resolvedWorktree); }}
            className="btn btn-primary"
          >
            {t('action.assign')}
          </button>
        </>
      }
    >
      <div className="space-y-4">
        {hasExistingWorktrees && (
          <>
            {/* Worktree picker */}
            <div>
              <label className="block text-xs font-medium text-main mb-3">
                {t('assign.selectWorktree')}
              </label>
              {worktrees.length > 5 && (
                <input
                  type="text"
                  className={`${inputClass} mb-2`}
                  placeholder={t('assign.filterPlaceholder')}
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                />
              )}
              <div className="max-h-48 overflow-y-auto rounded-md border border-slate-300 dark:border-[var(--border)] bg-white/60 dark:bg-transparent py-1">
                {availableWorktrees.length === 0 ? (
                  <div className="px-3 py-4 text-center text-xs text-slate-500 dark:text-subtle-ui">
                    {t('assign.noWorktrees')}
                  </div>
                ) : (
                  availableWorktrees.map(({ name, occupied }) => (
                    <button
                      key={name}
                      type="button"
                      disabled={occupied}
                      className={`w-full text-left px-3 py-1.5 text-xs font-mono transition-colors ${
                        occupied
                          ? 'text-slate-400 dark:text-subtle-ui cursor-not-allowed opacity-50'
                          : selected === name
                            ? 'bg-[var(--primary)]/15 text-[var(--primary)]'
                            : 'text-main hover:bg-slate-200/60 dark:hover:bg-white/5'
                      }`}
                      onClick={() => {
                        setSelected(name);
                        setCustomInput('');
                      }}
                    >
                      <span>{name}</span>
                      {occupied && (
                        <span className="ml-2 text-[10px] text-slate-500 dark:text-subtle-ui">
                          {t('assign.branchOccupied')}
                        </span>
                      )}
                    </button>
                  ))
                )}
              </div>
            </div>

            {/* Divider */}
            <div className="flex items-center gap-3 text-xs text-slate-500 dark:text-subtle-ui">
              <div className="flex-1 border-t border-slate-300 dark:border-[var(--border)]" />
              <span>{t('assign.orCreateNew')}</span>
              <div className="flex-1 border-t border-slate-300 dark:border-[var(--border)]" />
            </div>
          </>
        )}

        {/* Custom worktree input */}
        <div>
          {!hasExistingWorktrees && (
            <p className="mb-2 text-xs text-slate-500 dark:text-subtle-ui">{t('assign.noWorktrees')}</p>
          )}
          <input
            type="text"
            className={`${inputClass} font-mono text-xs`}
            placeholder={t('assign.newWorktreePlaceholder')}
            value={customInput}
            onChange={(e) => {
              setCustomInput(e.target.value);
              if (e.target.value.trim()) setSelected(null);
            }}
          />
        </div>
      </div>
    </Modal>
  );
}
