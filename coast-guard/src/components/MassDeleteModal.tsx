import { useState, useMemo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { SpinnerGap, CheckSquare, Square } from '@phosphor-icons/react';
import Modal from './Modal';

interface DeletableEntry {
  readonly name: string;
  readonly root: string | null;
}

interface MassDeleteModalProps {
  readonly open: boolean;
  readonly projects: readonly DeletableEntry[];
  readonly onDelete: (projects: readonly string[]) => void;
  readonly onClose: () => void;
  readonly deleting: boolean;
}

export default function MassDeleteModal({
  open,
  projects,
  onDelete,
  onClose,
  deleting,
}: MassDeleteModalProps) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const sortedProjects = useMemo(
    () => [...projects].sort((a, b) => a.name.localeCompare(b.name)),
    [projects],
  );

  const allSelected = sortedProjects.length > 0 && selected.size === sortedProjects.length;
  const noneSelected = selected.size === 0;

  const toggleProject = useCallback((name: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const toggleAll = useCallback(() => {
    if (allSelected) {
      setSelected(new Set());
    } else {
      setSelected(new Set(sortedProjects.map((p) => p.name)));
    }
  }, [allSelected, sortedProjects]);

  const handleClose = useCallback(() => {
    if (deleting) return;
    setSelected(new Set());
    onClose();
  }, [deleting, onClose]);

  const handleDelete = useCallback(() => {
    if (selected.size === 0) return;
    onDelete(Array.from(selected));
  }, [selected, onDelete]);

  return (
    <Modal
      open={open}
      title={t('projects.bulkDeleteTitle')}
      onClose={handleClose}
      wide
      actions={
        <>
          <button onClick={handleClose} className="btn btn-outline" disabled={deleting}>
            {t('action.cancel')}
          </button>
          <button
            onClick={handleDelete}
            className="btn btn-danger"
            disabled={noneSelected || deleting}
          >
            {deleting ? (
              <span className="flex items-center gap-1.5">
                <SpinnerGap size={14} className="animate-spin" />
                {t('projects.bulkDeleting')}
              </span>
            ) : (
              t('projects.bulkDeleteConfirm', { count: selected.size })
            )}
          </button>
        </>
      }
    >
      <div className="space-y-3">
        <p className="text-sm text-muted-ui">{t('projects.bulkDeleteBody')}</p>

        <div className="flex items-center justify-between pt-1">
          <button
            onClick={toggleAll}
            className="flex items-center gap-1.5 text-xs text-blue-600 hover:text-blue-700 dark:text-blue-400 dark:hover:text-blue-300 transition-colors"
            disabled={deleting}
          >
            {allSelected ? <CheckSquare size={14} weight="fill" /> : <Square size={14} />}
            {allSelected ? t('projects.bulkDeselectAll') : t('projects.bulkSelectAll')}
          </button>
          <span className="text-xs text-subtle-ui">
            {t('toolbar.selected', { count: selected.size })}
          </span>
        </div>

        <div className="max-h-64 overflow-y-auto -mx-1 px-1 space-y-1">
          {sortedProjects.map((p) => {
            const isSelected = selected.has(p.name);
            return (
              <label
                key={p.name}
                className={`flex items-center gap-3 px-3 py-2 rounded-lg cursor-pointer transition-colors ${
                  isSelected
                    ? 'bg-rose-50/60 dark:bg-rose-900/20 border border-rose-200/60 dark:border-rose-700/40'
                    : 'hover:bg-white/40 dark:hover:bg-white/5 border border-transparent'
                } ${deleting ? 'pointer-events-none opacity-60' : ''}`}
              >
                <input
                  type="checkbox"
                  checked={isSelected}
                  onChange={() => toggleProject(p.name)}
                  disabled={deleting}
                  className="sr-only"
                />
                {isSelected ? (
                  <CheckSquare size={18} weight="fill" className="text-rose-600 dark:text-rose-400 shrink-0" />
                ) : (
                  <Square size={18} className="text-subtle-ui shrink-0" />
                )}
                <span className="flex-1 min-w-0">
                  <span className="text-sm font-medium text-main block truncate">{p.name}</span>
                  {p.root != null && (
                    <span className="text-xs font-mono text-muted-ui block truncate">{p.root}</span>
                  )}
                </span>
              </label>
            );
          })}
        </div>
      </div>
    </Modal>
  );
}
