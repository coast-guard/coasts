import { useState, useMemo, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import Modal from './Modal';
import { api } from '../api/endpoints';
import type { BuildSummary } from '../types/api';

interface CreateCoastModalProps {
  readonly open: boolean;
  readonly project: string;
  readonly existingNames: ReadonlySet<string>;
  readonly builds?: readonly BuildSummary[];
  readonly worktrees: readonly string[];
  readonly occupiedWorktrees: ReadonlySet<string>;
  readonly onCreated: (name: string, worktree: string | null) => void;
  readonly onError?: (msg: string) => void;
  readonly onClose: () => void;
}

const NAME_RE = /^[a-z0-9][a-z0-9-]*$/;

const inputClass =
  'w-full h-9 px-3 text-sm rounded-md border border-[var(--border)] bg-[var(--surface-solid)] dark:bg-transparent text-main outline-none focus:border-[var(--primary)] placeholder:text-subtle-ui';

export default function CreateCoastModal({
  open, project, existingNames, builds = [], worktrees, occupiedWorktrees, onCreated, onError, onClose,
}: CreateCoastModalProps) {
  const { t } = useTranslation();
  const [coastName, setCoastName] = useState('');
  const [selectedWorktree, setSelectedWorktree] = useState<string | null>(null);
  const [customWorktree, setCustomWorktree] = useState('');
  const [selectedCoastfileType, setSelectedCoastfileType] = useState('default');
  const [worktreeFilter, setWorktreeFilter] = useState('');
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [danglingDetected, setDanglingDetected] = useState(false);

  useEffect(() => {
    if (!open) {
      setCoastName('');
      setSelectedWorktree(null);
      setCustomWorktree('');
      setSelectedCoastfileType('default');
      setWorktreeFilter('');
      setCreating(false);
      setError(null);
      setDanglingDetected(false);
    }
  }, [open]);

  const nameError = useMemo(() => {
    if (creating) return null;
    const trimmed = coastName.trim();
    if (!trimmed) return null;
    if (!NAME_RE.test(trimmed)) return t('create.nameInvalid');
    if (existingNames.has(trimmed)) return t('create.nameTaken', { name: trimmed });
    return null;
  }, [coastName, existingNames, creating, t]);

  const validName = coastName.trim().length > 0 && nameError == null;
  const resolvedWorktree = customWorktree.trim() || selectedWorktree;
  const availableTypes = useMemo(
    () => {
      const safeBuilds = Array.isArray(builds) ? builds : [];
      return (
      Array.from(
        new Set(safeBuilds.map((b) => b.coastfile_type ?? 'default')),
      ).sort((a, b) => {
        if (a === 'default' && b !== 'default') return -1;
        if (a !== 'default' && b === 'default') return 1;
        return a.localeCompare(b);
      })
      );
    },
    [builds],
  );
  const selectableTypes = useMemo(
    () => (availableTypes.length > 0 ? availableTypes : ['default']),
    [availableTypes],
  );

  const availableWorktrees = useMemo(() => {
    const lowerFilter = worktreeFilter.toLowerCase();
    return worktrees
      .filter((w) => !lowerFilter || w.toLowerCase().includes(lowerFilter))
      .map((w) => ({ name: w, occupied: occupiedWorktrees.has(w) }));
  }, [worktrees, occupiedWorktrees, worktreeFilter]);

  const hasExistingWorktrees = worktrees.length > 0;

  const fireRun = (forceRemoveDangling: boolean) => {
    setCreating(true);
    setError(null);
    setDanglingDetected(false);

    const trimmedName = coastName.trim();
    const worktreeArg = resolvedWorktree ?? null;
    let closed = false;
    let progressCount = 0;

    const closeOnce = () => {
      if (!closed) {
        closed = true;
        onCreated(trimmedName, worktreeArg);
      }
    };

    api.runInstance(
      project,
      coastName.trim(),
      resolvedWorktree ?? undefined,
      undefined,
      selectedCoastfileType === 'default'
        ? undefined
        : selectedCoastfileType,
      forceRemoveDangling,
      () => {
        progressCount++;
        // Close after the 3rd progress event — past the dangling-check
        // window (dangling errors arrive after exactly 2 progress events).
        if (progressCount >= 3) closeOnce();
      },
    ).then((result) => {
      if (result.error) {
        const msg = result.error.error ?? JSON.stringify(result.error);
        if (closed) {
          onError?.(msg);
          return;
        }
        setCreating(false);
        if (msg.includes('dangling Docker container')) {
          setDanglingDetected(true);
        }
        setError(msg);
      } else {
        closeOnce();
      }
    }).catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err);
      if (closed) {
        onError?.(msg);
        return;
      }
      setCreating(false);
      if (msg.includes('dangling Docker container')) {
        setDanglingDetected(true);
      }
      setError(msg);
    });
  };

  const handleCreate = () => {
    if (!validName || creating) return;
    fireRun(false);
  };

  const handleForceCreate = () => {
    if (!validName || creating) return;
    fireRun(true);
  };

  return (
    <Modal
      open={open}
      title={t('create.title')}
      onClose={onClose}
      actions={
        <>
          <button onClick={onClose} className="btn btn-outline" disabled={creating}>
            {t('action.cancel')}
          </button>
          {danglingDetected ? (
            <button
              disabled={creating}
              onClick={() => void handleForceCreate()}
              className="btn bg-amber-600 hover:bg-amber-700 text-white border border-amber-500"
            >
              {t('create.removeDanglingAndCreate', 'Remove & Create')}
            </button>
          ) : (
            <button
              disabled={!validName || creating}
              onClick={() => void handleCreate()}
              className="btn btn-primary"
            >
              {creating ? t('create.creating') : t('create.submit')}
            </button>
          )}
        </>
      }
    >
      <div className="space-y-4">
        {/* Coast name */}
        <div>
          <label className="block text-xs font-medium text-main mb-1.5">
            {t('create.nameLabel')}
          </label>
          <input
            type="text"
            className={`${inputClass} font-mono ${nameError ? '!border-rose-400 dark:!border-rose-500' : ''}`}
            placeholder={t('create.namePlaceholder')}
            value={coastName}
            onChange={(e) => setCoastName(e.target.value.toLowerCase())}
            disabled={creating}
            autoFocus
          />
          {nameError && (
            <p className="mt-1 text-xs text-rose-600 dark:text-rose-400">{nameError}</p>
          )}
        </div>

        {/* Optional Coastfile selection */}
        <div className="space-y-0.5">
          <label className="text-xs font-semibold text-main mb-2 block">
            {t('build.type')}:
          </label>
          <div className="flex flex-wrap gap-1.5 pt-0.5 pb-1.5">
            {selectableTypes.map((type) => {
              return (
                <button
                  key={type}
                  type="button"
                  onClick={() => setSelectedCoastfileType(type)}
                  disabled={creating}
                  className={`px-2.5 py-1 rounded-md text-[11px] font-mono border cursor-pointer transition-colors ${
                    selectedCoastfileType === type
                      ? 'bg-[var(--primary)] border-[var(--primary-strong)] text-white'
                      : 'bg-[var(--surface-muted)] border-[var(--border)] text-main hover:bg-[var(--surface-strong)]'
                  }`}
                >
                  {type}
                </button>
              );
            })}
          </div>
          <p className="mt-2 text-[11px] leading-5 text-subtle-ui">
            <span className="font-mono">
              {selectedCoastfileType === 'default'
                ? 'Coastfile'
                : `Coastfile.${selectedCoastfileType}`}
            </span>
          </p>
        </div>

        {/* Divider */}
        <div className="-mt-2 flex items-center gap-3 text-xs text-subtle-ui">
          <div className="flex-1 border-t border-[var(--border)]" />
          <span>{t('create.worktreeLabel')}</span>
          <div className="flex-1 border-t border-[var(--border)]" />
        </div>

        {/* Worktree picker */}
        {hasExistingWorktrees && (
          <div>
            {worktrees.length > 5 && (
              <input
                type="text"
                className={`${inputClass} mb-2`}
                placeholder={t('assign.filterPlaceholder')}
                value={worktreeFilter}
                onChange={(e) => setWorktreeFilter(e.target.value)}
                disabled={creating}
              />
            )}
            <div className="max-h-40 overflow-y-auto rounded-md border border-[var(--border)] bg-[var(--surface-muted)] dark:bg-transparent py-1">
              {availableWorktrees.length === 0 ? (
                <div className="px-3 py-4 text-center text-xs text-subtle-ui">
                  {t('assign.noWorktrees')}
                </div>
              ) : (
                availableWorktrees.map(({ name, occupied }) => (
                  <button
                    key={name}
                    type="button"
                    disabled={occupied || creating}
                    className={`w-full text-left px-3 py-1.5 text-xs font-mono transition-colors ${
                      occupied
                        ? 'text-subtle-ui cursor-not-allowed opacity-50'
                        : selectedWorktree === name
                          ? 'bg-[var(--primary)]/15 text-[var(--primary)]'
                          : 'text-main hover:bg-[var(--surface-hover)]'
                    }`}
                    onClick={() => {
                      setSelectedWorktree(selectedWorktree === name ? null : name);
                      setCustomWorktree('');
                    }}
                  >
                    <span>{name}</span>
                    {occupied && (
                      <span className="ml-2 text-[10px] text-subtle-ui">
                        {t('assign.branchOccupied')}
                      </span>
                    )}
                  </button>
                ))
              )}
            </div>
          </div>
        )}

        {/* Custom worktree input */}
        <div>
          {!hasExistingWorktrees && (
            <p className="mb-2 text-xs text-subtle-ui">{t('assign.noWorktrees')}</p>
          )}
          <input
            type="text"
            className={`${inputClass} font-mono text-xs`}
            placeholder={t('assign.newWorktreePlaceholder')}
            value={customWorktree}
            onChange={(e) => {
              setCustomWorktree(e.target.value);
              if (e.target.value.trim()) setSelectedWorktree(null);
            }}
            disabled={creating}
          />
        </div>

        {/* Dangling container warning */}
        {danglingDetected && (
          <div className="rounded-md border border-amber-400 dark:border-amber-600 bg-amber-50 dark:bg-amber-950/30 px-3 py-2.5 text-xs text-amber-800 dark:text-amber-300">
            <p className="font-semibold mb-1">{t('create.danglingTitle', 'Dangling container detected')}</p>
            <p>{t('create.danglingDescription', 'A Docker container with this name already exists from a previous failed run. Click "Remove & Create" to clean it up and proceed.')}</p>
          </div>
        )}

        {/* Error display */}
        {error && !danglingDetected && (
          <p className="text-xs text-rose-600 dark:text-rose-400 whitespace-pre-wrap">{error}</p>
        )}
      </div>
    </Modal>
  );
}
