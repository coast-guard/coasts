import { useState, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import type { BuildSummary } from '../types/api';
import { api } from '../api/endpoints';
import { ApiError } from '../api/client';
import { useRemovingProjects } from '../providers/RemovingProjectsProvider';
import DataTable, { type Column } from './DataTable';
import Toolbar, { type ToolbarAction } from './Toolbar';
import ConfirmModal from './ConfirmModal';
import Modal from './Modal';

function formatBytes(bytes: number): string {
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  if (bytes >= 1_048_576) return `${Math.round(bytes / 1_048_576)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

function relativeTime(ts: string, t: ReturnType<typeof useTranslation>['t']): string {
  const date = new Date(ts);
  if (isNaN(date.getTime())) return ts;
  const seconds = Math.floor((Date.now() - date.getTime()) / 1000);
  if (seconds < 60) return t('time.justNow');
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return minutes === 1 ? t('time.minuteAgo') : t('time.minutesAgo', { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return hours === 1 ? t('time.hourAgo') : t('time.hoursAgo', { count: hours });
  const days = Math.floor(hours / 24);
  if (days < 30) return days === 1 ? t('time.dayAgo') : t('time.daysAgo', { count: days });
  return t('time.monthsAgo', { count: Math.floor(days / 30) });
}

interface BuildsListPanelProps {
  readonly project: string;
  readonly builds: readonly BuildSummary[];
  readonly t: ReturnType<typeof useTranslation>['t'];
  readonly navigate: (path: string) => void;
}

export default function BuildsListPanel({ project, builds, t, navigate }: BuildsListPanelProps) {
  const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(new Set());
  const [remoteSelectedIds, setRemoteSelectedIds] = useState<ReadonlySet<string>>(new Set());
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [confirmRemoteRemove, setConfirmRemoteRemove] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const queryClient = useQueryClient();
  const { removingBuilds } = useRemovingProjects();

  const allSorted = useMemo(
    () => [...builds].sort((a, b) => {
      if (a.is_latest && !b.is_latest) return -1;
      if (!a.is_latest && b.is_latest) return 1;
      const ta = a.build_timestamp ? new Date(a.build_timestamp).getTime() : 0;
      const tb = b.build_timestamp ? new Date(b.build_timestamp).getTime() : 0;
      return tb - ta;
    }),
    [builds],
  );

  const sorted = useMemo(() => allSorted.filter((b) => !b.is_remote), [allSorted]);
  const remoteBuilds = useMemo(() => allSorted.filter((b) => b.is_remote), [allSorted]);
  const remoteArchGroups = useMemo(() => {
    const archSet = Array.from(new Set(remoteBuilds.map((b) => b.arch ?? 'unknown'))).sort();
    return archSet.map((arch) => {
      const rows = remoteBuilds.filter((b) => (b.arch ?? 'unknown') === arch);
      if (rows.length > 0 && !rows.some((b) => b.is_latest)) {
        rows[0] = { ...rows[0], is_latest: true } as BuildSummary;
      }
      return { arch, rows };
    });
  }, [remoteBuilds]);
  const orderedTypes = useMemo(() => {
    const types = Array.from(
      new Set(sorted.map((b) => b.coastfile_type ?? 'default')),
    ).sort((a, b) => a.localeCompare(b));
    const withoutDefault = types.filter((t) => t !== 'default');
    return ['default', ...withoutDefault.filter((t) => t !== 'default')].filter(
      (t, i, arr) => arr.indexOf(t) === i && types.includes(t),
    );
  }, [sorted]);
  const hasMultipleTypes = orderedTypes.length > 1;
  const groupedByType = useMemo(
    () =>
      orderedTypes.map((type) => ({
        type,
        rows: sorted.filter((b) => (b.coastfile_type ?? 'default') === type),
      })),
    [orderedTypes, sorted],
  );

  const selectedCount = selectedIds.size;
  const selectedBuildIds = useMemo(
    () => sorted.filter((b) => selectedIds.has(b.build_id ?? 'legacy')).map((b) => b.build_id).filter((id): id is string => id != null),
    [sorted, selectedIds],
  );
  const hasLatestInUseSelected = useMemo(
    () => sorted.some((b) => b.is_latest && (b.instances_using ?? 0) > 0 && selectedIds.has(b.build_id ?? 'legacy')),
    [sorted, selectedIds],
  );
  const hasInUseSelected = useMemo(
    () => sorted.some((b) => (b.instances_using ?? 0) > 0 && selectedIds.has(b.build_id ?? 'legacy')),
    [sorted, selectedIds],
  );

  const handleRemove = useCallback(async () => {
    setConfirmRemove(false);
    const idsToRemove = selectedBuildIds.filter(
      (id) => {
        const build = sorted.find((b) => b.build_id === id);
        return build && !(build.instances_using ?? 0);
      },
    );
    if (idsToRemove.length === 0) return;
    try {
      const result = await api.rmBuild(project, idsToRemove);
      if (result.error) {
        setError(result.error.error);
      } else {
        setSelectedIds(new Set());
      }
      void queryClient.invalidateQueries({ queryKey: ['buildsLs'] });
    } catch (e) {
      setError(e instanceof ApiError ? e.body.error : String(e));
    }
  }, [selectedBuildIds, sorted, project, queryClient]);

  const remoteSelectedCount = remoteSelectedIds.size;
  const remoteSelectedBuildIds = useMemo(
    () => remoteBuilds.filter((b) => remoteSelectedIds.has(b.build_id ?? 'legacy')).map((b) => b.build_id).filter((id): id is string => id != null),
    [remoteBuilds, remoteSelectedIds],
  );

  const handleRemoteRemove = useCallback(async () => {
    setConfirmRemoteRemove(false);
    if (remoteSelectedBuildIds.length === 0) return;
    try {
      const result = await api.rmBuild(project, remoteSelectedBuildIds);
      if (result.error) {
        setError(result.error.error);
      } else {
        setRemoteSelectedIds(new Set());
      }
      void queryClient.invalidateQueries({ queryKey: ['buildsLs'] });
    } catch (e) {
      setError(e instanceof ApiError ? e.body.error : String(e));
    }
  }, [remoteSelectedBuildIds, project, queryClient]);

  const remoteToolbarActions: readonly ToolbarAction[] = useMemo(
    () => [
      {
        label: t('action.remove'),
        variant: 'danger' as const,
        onClick: () => setConfirmRemoteRemove(true),
      },
    ],
    [t],
  );

  const toolbarActions: readonly ToolbarAction[] = useMemo(
    () => [
      {
        label: t('action.remove'),
        variant: 'danger' as const,
        onClick: () => setConfirmRemove(true),
      },
    ],
    [t],
  );

  const columns: readonly Column<BuildSummary>[] = useMemo(
    () => {
      const cols: Column<BuildSummary>[] = [
      {
        key: 'buildId',
        header: t('build.buildId'),
        className: 'w-auto',
        headerClassName: 'w-auto',
        render: (b) => (
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-xs text-[var(--primary)]">{b.build_id ?? '—'}</span>
            {b.is_latest && (
              <span className="inline-block whitespace-nowrap px-1.5 py-0.5 rounded text-[10px] font-medium bg-green-500/15 text-green-700 dark:text-green-300">
                {t('build.latest')}
              </span>
            )}
            {(b.instances_using ?? 0) > 0 && (
              <span className="inline-block whitespace-nowrap px-1.5 py-0.5 rounded text-[10px] font-medium bg-[var(--primary)]/15 text-[var(--primary-strong)] dark:text-[var(--primary)]">
                {t('build.inUse', { count: b.instances_using ?? 0 })}
              </span>
            )}
            {b.build_id != null && removingBuilds.has(b.build_id) && (
              <span className="inline-flex items-center gap-1 whitespace-nowrap px-1.5 py-0.5 rounded text-[10px] font-medium bg-rose-500/15 text-rose-700 dark:text-rose-300">
                <span className="inline-block h-1.5 w-1.5 rounded-full bg-rose-500 animate-pulse" />
                {t('build.removing')}
              </span>
            )}
          </div>
        ),
      },
      {
        key: 'built',
        header: t('build.built'),
        className: 'w-44',
        headerClassName: 'w-44',
        render: (b) => (
          <span className="text-subtle-ui">
            {b.build_timestamp != null ? relativeTime(b.build_timestamp, t) : '—'}
          </span>
        ),
      },
      {
        key: 'images',
        header: t('build.images'),
        className: 'w-24',
        headerClassName: 'w-24',
        render: (b) => <>{b.images_built}</>,
      },
      {
        key: 'secrets',
        header: t('build.secretsLabel'),
        className: 'w-24',
        headerClassName: 'w-24',
        render: (b) => <>{b.secrets_count}</>,
      },
      {
        key: 'cache',
        header: t('build.cache'),
        className: 'w-28',
        headerClassName: 'w-28',
        render: (b) => <>{formatBytes(b.cache_size_bytes)}</>,
      },
      ];
      if (!hasMultipleTypes) {
        cols.splice(1, 0, {
          key: 'type',
          header: t('build.type'),
          className: 'w-36',
          headerClassName: 'w-36',
          render: (b) => (
            <span className="text-subtle-ui">{b.coastfile_type ?? 'default'}</span>
          ),
        });
      }
      return cols;
    },
    [t, hasMultipleTypes],
  );

  const remoteColumns: readonly Column<BuildSummary>[] = useMemo(
    () => [
      {
        key: 'buildId',
        header: t('build.buildId'),
        className: 'w-auto',
        headerClassName: 'w-auto',
        render: (b: BuildSummary) => (
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-xs text-[var(--primary)]">{b.build_id ?? '—'}</span>
            {b.is_latest && (
              <span className="inline-block whitespace-nowrap px-1.5 py-0.5 rounded text-[10px] font-medium bg-green-500/15 text-green-700 dark:text-green-300">
                {t('build.latest')}
              </span>
            )}
            {(b.instances_using ?? 0) > 0 && (
              <span className="inline-block whitespace-nowrap px-1.5 py-0.5 rounded text-[10px] font-medium bg-[var(--primary)]/15 text-[var(--primary-strong)] dark:text-[var(--primary)]">
                {t('build.inUse', { count: b.instances_using ?? 0 })}
              </span>
            )}
          </div>
        ),
      },
      {
        key: 'arch',
        header: t('build.remoteArch'),
        className: 'w-28',
        headerClassName: 'w-28',
        render: (b: BuildSummary) => (
          <span className="font-mono text-xs">{b.arch ?? '—'}</span>
        ),
      },
      {
        key: 'built',
        header: t('build.built'),
        className: 'w-44',
        headerClassName: 'w-44',
        render: (b: BuildSummary) => (
          <span className="text-subtle-ui">
            {b.build_timestamp != null ? relativeTime(b.build_timestamp, t) : '—'}
          </span>
        ),
      },
      {
        key: 'images',
        header: t('build.images'),
        className: 'w-24',
        headerClassName: 'w-24',
        render: (b: BuildSummary) => <>{b.images_built}</>,
      },
      {
        key: 'cache',
        header: t('build.cache'),
        className: 'w-28',
        headerClassName: 'w-28',
        render: (b: BuildSummary) => <>{formatBytes(b.cache_size_bytes)}</>,
      },
    ],
    [t],
  );

  if (builds.length === 0) {
    return (
      <section className="mt-4">
        <div className="glass-panel p-6 text-sm text-subtle-ui">
          {t('build.noBuild')}
        </div>
      </section>
    );
  }

  return (
    <section className="mt-1">
      <div className="glass-panel overflow-hidden">
        <Toolbar actions={toolbarActions} selectedCount={selectedCount} />
        {!hasMultipleTypes ? (
          <DataTable
            columns={columns}
            data={sorted}
            getRowId={(b) => b.build_id ?? 'legacy'}
            selectable
            selectedIds={selectedIds}
            onSelectionChange={setSelectedIds}
            onRowClick={(b) => navigate(`/project/${project}/builds/${b.build_id ?? 'latest'}`)}
            emptyMessage={t('build.noBuild')}
          />
        ) : (
          <div className="p-4 space-y-4">
            {groupedByType
              .filter((g) => g.rows.length > 0)
              .map((group) => {
                const groupIds = group.rows.map((b) => b.build_id ?? 'legacy');
                return (
                  <div key={group.type} className="rounded-lg border border-[var(--border)] overflow-hidden">
                    <div className="px-4 py-2 text-xs font-semibold uppercase tracking-wide text-subtle-ui bg-[var(--surface-muted)]">
                      {t('build.type')}: <span className="font-mono normal-case">{group.type}</span>
                    </div>
                    <DataTable
                      columns={columns}
                      data={group.rows}
                      getRowId={(b) => b.build_id ?? 'legacy'}
                      tableClassName="table-fixed"
                      selectable
                      selectedIds={selectedIds}
                      onSelectionChange={(next) => {
                        setSelectedIds((prev) => {
                          const nextSet = new Set(next);
                          const sectionOnly = [...nextSet].every((id) => groupIds.includes(id));
                          if (!sectionOnly) return nextSet;
                          const merged = new Set(prev);
                          const allSectionSelectedBefore = groupIds.every((id) => prev.has(id));
                          if (nextSet.size === 0 && allSectionSelectedBefore) {
                            groupIds.forEach((id) => merged.delete(id));
                            return merged;
                          }
                          if (nextSet.size === groupIds.length) {
                            groupIds.forEach((id) => merged.add(id));
                            return merged;
                          }
                          return nextSet;
                        });
                      }}
                      onRowClick={(b) => navigate(`/project/${project}/builds/${b.build_id ?? 'latest'}`)}
                      emptyMessage={t('build.noBuild')}
                    />
                  </div>
                );
              })}
          </div>
        )}
      </div>

      {remoteArchGroups.length > 0 && (
        <div className="mt-4">
          <h3 className="text-xs font-semibold uppercase tracking-wide text-subtle-ui mb-2">
            {t('build.remoteBuilds')}
          </h3>
          <div className="space-y-3">
            {remoteArchGroups.map((group) => {
              const groupIds = group.rows.map((b) => b.build_id ?? 'legacy');
              return (
                <div key={group.arch} className="glass-panel overflow-hidden">
                  <Toolbar
                    actions={remoteToolbarActions}
                    selectedCount={remoteSelectedCount}
                    memorySummary={`${t('build.remoteArch')}: `}
                    memoryHighlight={`${group.arch} (${group.rows.length})`}
                  />
                  <DataTable
                    columns={remoteColumns}
                    data={group.rows}
                    getRowId={(b) => b.build_id ?? 'legacy'}
                    selectable
                    selectedIds={remoteSelectedIds}
                    onSelectionChange={(next) => {
                      setRemoteSelectedIds((prev) => {
                        const nextSet = new Set(next);
                        const sectionOnly = [...nextSet].every((id) => groupIds.includes(id));
                        if (!sectionOnly) return nextSet;
                        const merged = new Set(prev);
                        const allSelectedBefore = groupIds.every((id) => prev.has(id));
                        if (nextSet.size === 0 && allSelectedBefore) {
                          groupIds.forEach((id) => merged.delete(id));
                          return merged;
                        }
                        if (nextSet.size === groupIds.length) {
                          groupIds.forEach((id) => merged.add(id));
                          return merged;
                        }
                        return nextSet;
                      });
                    }}
                    onRowClick={(b) => navigate(`/project/${project}/builds/${b.build_id ?? 'latest'}`)}
                    emptyMessage={t('build.noBuild')}
                  />
                </div>
              );
            })}
          </div>
        </div>
      )}

      <ConfirmModal
        open={confirmRemove}
        title={t('build.removeTitle')}
        body={
          [
            t('build.removeConfirm', { count: selectedBuildIds.length }),
            hasLatestInUseSelected ? t('build.latestSkipNote') : '',
            hasInUseSelected ? t('build.inUseSkipNote') : '',
          ].filter(Boolean).join(' ')
        }
        onConfirm={() => void handleRemove()}
        onCancel={() => setConfirmRemove(false)}
      />

      <ConfirmModal
        open={confirmRemoteRemove}
        title={t('build.removeTitle')}
        body={t('build.removeConfirm', { count: remoteSelectedBuildIds.length })}
        onConfirm={() => void handleRemoteRemove()}
        onCancel={() => setConfirmRemoteRemove(false)}
      />

      {error != null && (
        <Modal open onClose={() => setError(null)} title={t('error.title')}>
          <p className="text-sm text-rose-600 dark:text-rose-400">{error}</p>
        </Modal>
      )}
    </section>
  );
}
