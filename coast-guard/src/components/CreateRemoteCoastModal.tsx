import { useState, useMemo, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import Modal from './Modal';
import { api } from '../api/endpoints';
import { useRemotesLs, useBuildsLs } from '../api/hooks';
import { SpinnerGap } from '@phosphor-icons/react';
import type { BuildProgressEvent } from '../types/api';

interface CreateRemoteCoastModalProps {
  readonly open: boolean;
  readonly project: string;
  readonly existingNames: ReadonlySet<string>;
  readonly worktrees: readonly string[];
  readonly occupiedWorktrees: ReadonlySet<string>;
  readonly currentBranch?: string | null | undefined;
  readonly onCreated: (name: string, worktree: string | null) => void;
  readonly onError?: (msg: string) => void;
  readonly onClose: () => void;
}

const NAME_RE = /^[a-z0-9][a-z0-9-]*$/;

const inputClass =
  'w-full h-9 px-3 text-sm rounded-md border border-[var(--border)] bg-[var(--surface-solid)] dark:bg-transparent text-main outline-none focus:border-[var(--primary)] placeholder:text-subtle-ui';

export default function CreateRemoteCoastModal({
  open, project, existingNames, worktrees, occupiedWorktrees, currentBranch, onCreated, onError, onClose,
}: CreateRemoteCoastModalProps) {
  const { t } = useTranslation();
  const { data: remotesData } = useRemotesLs();
  const remotes = remotesData?.remotes ?? [];
  const { data: buildsData } = useBuildsLs(project);

  const [coastName, setCoastName] = useState('');
  const [selectedRemote, setSelectedRemote] = useState('');
  const [remoteArch, setRemoteArch] = useState<string | null>(null);
  const [remoteTypes, setRemoteTypes] = useState<string[]>([]);
  const [selectedType, setSelectedType] = useState('remote');
  const [selectedWorktree, setSelectedWorktree] = useState<string | null>(null);
  const [customWorktree, setCustomWorktree] = useState('');
  const [worktreeFilter, setWorktreeFilter] = useState('');
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [danglingDetected, setDanglingDetected] = useState(false);

  useEffect(() => {
    if (!open) {
      setCoastName('');
      setSelectedRemote('');
      setRemoteArch(null);
      setRemoteTypes([]);
      setSelectedType('remote');
      setSelectedWorktree(null);
      setCustomWorktree('');
      setWorktreeFilter('');
      setCreating(false);
      setError(null);
      setDanglingDetected(false);
    }
  }, [open]);

  useEffect(() => {
    if (!open || !project) return;
    const builds = buildsData?.builds ?? [];
    const remoteBuilds = builds.filter((b) => b.is_remote);

    const typesWithArch = remoteArch
      ? Array.from(new Set(
          remoteBuilds
            .filter((b) => (b.arch ?? 'unknown') === remoteArch)
            .map((b) => b.coastfile_type ?? 'remote')
        ))
      : Array.from(new Set(remoteBuilds.map((b) => b.coastfile_type ?? 'remote')));

    typesWithArch.sort((a, b) => {
      if (a === 'remote') return -1;
      if (b === 'remote') return 1;
      return a.localeCompare(b);
    });

    setRemoteTypes(typesWithArch.length > 0 ? typesWithArch : ['remote']);
    if (typesWithArch.length > 0 && typesWithArch[0]) {
      setSelectedType(typesWithArch[0]);
    }
  }, [open, project, buildsData, remoteArch]);

  useEffect(() => {
    if (open && remotes.length === 1 && remotes[0] && !selectedRemote) {
      setSelectedRemote(remotes[0].name);
    }
  }, [open, remotes, selectedRemote]);

  useEffect(() => {
    if (!selectedRemote) {
      setRemoteArch(null);
      return;
    }
    setRemoteArch(null);
    api.remoteArch(selectedRemote)
      .then((resp) => setRemoteArch(resp.arch))
      .catch(() => setRemoteArch('unknown'));
  }, [selectedRemote]);

  const nameError = useMemo(() => {
    if (creating) return null;
    const trimmed = coastName.trim();
    if (!trimmed) return null;
    if (!NAME_RE.test(trimmed)) return t('create.nameInvalid');
    if (existingNames.has(trimmed)) return t('create.nameTaken', { name: trimmed });
    return null;
  }, [coastName, existingNames, creating, t]);

  const validName = coastName.trim().length > 0 && nameError == null;
  const hasCompatibleBuild = useMemo(() => {
    if (!remoteArch || !buildsData) return true;
    const remoteBuilds = (buildsData.builds ?? []).filter((b) => b.is_remote);
    return remoteBuilds.some((b) => (b.arch ?? 'unknown') === remoteArch);
  }, [remoteArch, buildsData]);
  const resolvedWorktree = customWorktree.trim() || selectedWorktree;

  const availableWorktrees = useMemo(() => {
    const lowerFilter = worktreeFilter.toLowerCase();
    return worktrees
      .filter((w) => !lowerFilter || w.toLowerCase().includes(lowerFilter))
      .map((w) => ({ name: w, occupied: occupiedWorktrees.has(w) }));
  }, [worktrees, occupiedWorktrees, worktreeFilter]);

  const hasExistingWorktrees = worktrees.length > 0;

  const fireRun = (forceRemoveDangling: boolean) => {
    if (!validName || !selectedRemote || creating) return;
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
      trimmedName,
      resolvedWorktree ?? undefined,
      undefined,
      selectedType,
      forceRemoveDangling,
      (event: BuildProgressEvent) => {
        progressCount++;
        if (event.step === 'Queued' || progressCount >= 3) {
          closeOnce();
        }
      },
      currentBranch,
      selectedRemote,
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

  return (
    <Modal
      open={open}
      title={t('create.remoteTitle')}
      onClose={onClose}
      actions={
        <>
          <button onClick={onClose} className="btn btn-outline" disabled={creating}>
            {t('action.cancel')}
          </button>
          {danglingDetected ? (
            <button
              disabled={creating}
              onClick={() => fireRun(true)}
              className="btn bg-amber-600 hover:bg-amber-700 text-white border border-amber-500"
            >
              {t('create.removeDanglingAndCreate', 'Remove & Create')}
            </button>
          ) : (
            <button
              disabled={!validName || !selectedRemote || !hasCompatibleBuild || creating}
              onClick={() => fireRun(false)}
              className="btn btn-primary"
            >
              {creating ? t('create.creating') : t('create.submit')}
            </button>
          )}
        </>
      }
    >
      <div className="space-y-4">
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

        <div>
          <label className="text-xs font-semibold text-main mb-2 block">
            Coastfile
          </label>
          <div className="flex flex-wrap gap-1.5 pt-0.5 pb-0.5">
            {remoteTypes.map((ct) => (
              <button
                key={ct}
                type="button"
                onClick={() => setSelectedType(ct)}
                disabled={creating}
                className={`px-2.5 py-1 rounded-md text-[11px] font-mono border cursor-pointer transition-colors ${
                  selectedType === ct
                    ? 'bg-[var(--primary)] border-[var(--primary-strong)] text-white'
                    : 'bg-[var(--surface-muted)] border-[var(--border)] text-main hover:bg-[var(--surface-strong)]'
                }`}
              >
                Coastfile.{ct}
              </button>
            ))}
          </div>
        </div>

        <div>
          <label className="text-xs font-semibold text-main mb-2 block">
            {t('build.selectRemote')}
          </label>
          <div className="flex flex-wrap gap-1.5">
            {remotes.map((r) => (
              <button
                key={r.name}
                type="button"
                onClick={() => setSelectedRemote(r.name)}
                disabled={creating}
                className={`px-3 py-1.5 rounded-md text-xs font-mono border transition-colors ${
                  selectedRemote === r.name
                    ? 'bg-[var(--primary)] border-[var(--primary-strong)] text-white'
                    : 'bg-[var(--surface-muted)] border-[var(--border)] text-main hover:bg-[var(--surface-strong)]'
                }`}
              >
                {r.name}
              </button>
            ))}
          </div>

          {selectedRemote && (
            <>
              <div className="mt-2 flex items-center gap-3 text-xs text-subtle-ui">
                <span>
                  {remotes.find((r) => r.name === selectedRemote)?.user}@
                  {remotes.find((r) => r.name === selectedRemote)?.host}:
                  {remotes.find((r) => r.name === selectedRemote)?.port}
                </span>
                {remoteArch && (
                  <>
                    <span className="text-[var(--border)]">|</span>
                    <span>
                      {t('build.remoteArch')}:{' '}
                      <strong className="text-main font-mono">{remoteArch}</strong>
                    </span>
                  </>
                )}
                {!remoteArch && (
                  <SpinnerGap size={14} className="animate-spin text-subtle-ui" />
                )}
              </div>
              {remoteArch && !hasCompatibleBuild && (
                <p className="text-xs text-amber-600 dark:text-amber-400 mt-1">
                  No builds for {remoteArch}. Run <code className="font-mono bg-[var(--surface-muted)] px-1 rounded">coast build --type remote --remote {selectedRemote}</code> first.
                </p>
              )}
            </>
          )}
        </div>

        <div className="-mt-2 flex items-center gap-3 text-xs text-subtle-ui">
          <div className="flex-1 border-t border-[var(--border)]" />
          <span>{t('create.worktreeLabel')}</span>
          <div className="flex-1 border-t border-[var(--border)]" />
        </div>

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

        {danglingDetected && (
          <div className="rounded-md border border-amber-400 dark:border-amber-600 bg-amber-50 dark:bg-amber-950/30 px-3 py-2.5 text-xs text-amber-800 dark:text-amber-300">
            <p className="font-semibold mb-1">{t('create.danglingTitle', 'Dangling container detected')}</p>
            <p>{t('create.danglingDescription', 'A Docker container with this name already exists from a previous failed run. Click "Remove & Create" to clean it up and proceed.')}</p>
          </div>
        )}

        {error && !danglingDetected && (
          <p className="text-xs text-rose-600 dark:text-rose-400 whitespace-pre-wrap">{error}</p>
        )}
      </div>
    </Modal>
  );
}
