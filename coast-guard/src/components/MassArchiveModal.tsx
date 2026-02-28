import { useState, useMemo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { SpinnerGap, CheckSquare, Square } from '@phosphor-icons/react';
import Modal from './Modal';

interface ProjectEntry {
  readonly name: string;
  readonly runningCount: number;
  readonly stoppedCount: number;
  readonly sharedTotal: number;
}

interface MassArchiveModalProps {
  readonly open: boolean;
  readonly projects: readonly ProjectEntry[];
  readonly onArchive: (projects: readonly string[]) => void;
  readonly onClose: () => void;
  readonly archiving: boolean;
}

export default function MassArchiveModal({
  open,
  projects,
  onArchive,
  onClose,
  archiving,
}: MassArchiveModalProps) {
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
    if (archiving) return;
    setSelected(new Set());
    onClose();
  }, [archiving, onClose]);

  const handleArchive = useCallback(() => {
    if (selected.size === 0) return;
    onArchive(Array.from(selected));
  }, [selected, onArchive]);

  const activeCount = useMemo(
    () =>
      sortedProjects
        .filter((p) => selected.has(p.name))
        .reduce((sum, p) => sum + p.runningCount + p.sharedTotal, 0),
    [sortedProjects, selected],
  );

  return (
    <Modal
      open={open}
      title={t('projects.bulkArchiveTitle')}
      onClose={handleClose}
      wide
      actions={
        <>
          <button onClick={handleClose} className="btn btn-outline" disabled={archiving}>
            {t('action.cancel')}
          </button>
          <button
            onClick={handleArchive}
            className="btn btn-danger"
            disabled={noneSelected || archiving}
          >
            {archiving ? (
              <span className="flex items-center gap-1.5">
                <SpinnerGap size={14} className="animate-spin" />
                {t('projects.bulkArchiving')}
              </span>
            ) : (
              t('projects.bulkArchiveConfirm', { count: selected.size })
            )}
          </button>
        </>
      }
    >
      <div className="space-y-3">
        <p className="text-sm text-muted-ui">{t('projects.bulkArchiveBody')}</p>

        {activeCount > 0 && (
          <p className="text-xs text-amber-600 dark:text-amber-400">
            {t('projects.bulkArchiveWarning', { count: activeCount })}
          </p>
        )}

        <div className="flex items-center justify-between pt-1">
          <button
            onClick={toggleAll}
            className="flex items-center gap-1.5 text-xs text-blue-600 hover:text-blue-700 dark:text-blue-400 dark:hover:text-blue-300 transition-colors"
            disabled={archiving}
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
                    ? 'bg-blue-50/60 dark:bg-blue-900/20 border border-blue-200/60 dark:border-blue-700/40'
                    : 'hover:bg-white/40 dark:hover:bg-white/5 border border-transparent'
                } ${archiving ? 'pointer-events-none opacity-60' : ''}`}
              >
                <input
                  type="checkbox"
                  checked={isSelected}
                  onChange={() => toggleProject(p.name)}
                  disabled={archiving}
                  className="sr-only"
                />
                {isSelected ? (
                  <CheckSquare size={18} weight="fill" className="text-blue-600 dark:text-blue-400 shrink-0" />
                ) : (
                  <Square size={18} className="text-subtle-ui shrink-0" />
                )}
                <span className="flex-1 min-w-0">
                  <span className="text-sm font-medium text-main block truncate">{p.name}</span>
                  <span className="text-xs text-muted-ui flex items-center gap-2">
                    {p.runningCount > 0 && (
                      <span className="flex items-center gap-1">
                        <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                        {t('projects.runningCount', { count: p.runningCount })}
                      </span>
                    )}
                    {p.stoppedCount > 0 && (
                      <span className="flex items-center gap-1">
                        <span className="h-1.5 w-1.5 rounded-full bg-rose-500" />
                        {t('projects.stoppedCount', { count: p.stoppedCount })}
                      </span>
                    )}
                    {p.sharedTotal > 0 && (
                      <span>{t('projects.sharedCount', { count: p.sharedTotal })}</span>
                    )}
                    {p.runningCount === 0 && p.stoppedCount === 0 && p.sharedTotal === 0 && (
                      <span>{t('projects.noCoasts')}</span>
                    )}
                  </span>
                </span>
              </label>
            );
          })}
        </div>
      </div>
    </Modal>
  );
}
