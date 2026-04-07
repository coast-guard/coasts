import { useState, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import type { RemoteEntry, RemoteStats } from '../types/api';
import { api } from '../api/endpoints';
import { ApiError } from '../api/client';
import { qk, useRemoteStats } from '../api/hooks';
import DataTable, { type Column } from './DataTable';
import Toolbar, { type ToolbarAction } from './Toolbar';
import ConfirmModal from './ConfirmModal';
import Modal from './Modal';

function formatSize(bytes: number): string {
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  if (bytes >= 1_048_576) return `${Math.round(bytes / 1_048_576)} MB`;
  return `${Math.round(bytes / 1024)} KB`;
}

function formatMemory(s: RemoteStats): string {
  return `${formatSize(s.used_memory_bytes)} / ${formatSize(s.total_memory_bytes)}`;
}

function formatDisk(s: RemoteStats): string {
  return `${formatSize(s.used_disk_bytes)} / ${formatSize(s.total_disk_bytes)}`;
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

function keyBasename(path: string | null): string {
  if (path == null) return '—';
  const parts = path.replace(/\\/g, '/').split('/');
  return parts[parts.length - 1] ?? path;
}

interface RemotesListPanelProps {
  readonly project: string;
  readonly remotes: readonly RemoteEntry[];
  readonly navigate: (path: string) => void;
}

export default function RemotesListPanel({ project, remotes, navigate }: RemotesListPanelProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: statsData } = useRemoteStats();
  const statsMap = statsData?.stats ?? {};
  const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(new Set());
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleRemove = useCallback(async () => {
    setConfirmRemove(false);
    const names = [...selectedIds];
    try {
      for (const name of names) {
        await api.remoteRm(name);
      }
      setSelectedIds(new Set());
      void queryClient.invalidateQueries({ queryKey: qk.remotesLs() });
    } catch (e) {
      setError(e instanceof ApiError ? e.body.error : String(e));
    }
  }, [selectedIds, queryClient]);

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

  const columns: readonly Column<RemoteEntry>[] = useMemo(
    () => [
      {
        key: 'name',
        header: t('remote.name'),
        className: 'w-40',
        headerClassName: 'w-40',
        render: (r) => (
          <span className="font-mono text-xs text-[var(--primary)]">{r.name}</span>
        ),
      },
      {
        key: 'host',
        header: t('remote.host'),
        className: 'w-auto',
        headerClassName: 'w-auto',
        render: (r) => (
          <span className="font-mono text-xs">{r.user}@{r.host}:{r.port}</span>
        ),
      },
      {
        key: 'sshKey',
        header: t('remote.sshKey'),
        className: 'w-40',
        headerClassName: 'w-40',
        render: (r) => (
          <span className="text-subtle-ui text-xs" title={r.ssh_key ?? undefined}>
            {keyBasename(r.ssh_key)}
          </span>
        ),
      },
      {
        key: 'sync',
        header: t('remote.sync'),
        className: 'w-28',
        headerClassName: 'w-28',
        render: (r) => <span className="text-subtle-ui">{r.sync_strategy}</span>,
      },
      {
        key: 'memory',
        header: t('remote.memory'),
        className: 'w-36',
        headerClassName: 'w-36',
        render: (r) => {
          const s = statsMap[r.name];
          return <span className="text-subtle-ui text-xs">{s ? formatMemory(s) : '—'}</span>;
        },
      },
      {
        key: 'cpu',
        header: t('remote.cpu'),
        className: 'w-24',
        headerClassName: 'w-24',
        render: (r) => {
          const s = statsMap[r.name];
          return <span className="text-subtle-ui text-xs">{s ? `${s.cpu_count} @ ${s.cpu_usage_percent.toFixed(0)}%` : '—'}</span>;
        },
      },
      {
        key: 'disk',
        header: t('remote.disk'),
        className: 'w-36',
        headerClassName: 'w-36',
        render: (r) => {
          const s = statsMap[r.name];
          return <span className="text-subtle-ui text-xs">{s ? formatDisk(s) : '—'}</span>;
        },
      },
      {
        key: 'added',
        header: t('remote.added'),
        className: 'w-36',
        headerClassName: 'w-36',
        render: (r) => (
          <span className="text-subtle-ui">{relativeTime(r.created_at, t)}</span>
        ),
      },
    ],
    [t, statsMap],
  );

  if (remotes.length === 0) {
    return (
      <section className="mt-4">
        <div className="glass-panel p-6 text-sm text-subtle-ui">
          {t('remote.noRemotes')}
        </div>
      </section>
    );
  }

  return (
    <section className="mt-1">
      <div className="glass-panel overflow-hidden">
        <Toolbar actions={toolbarActions} selectedCount={selectedIds.size} />
        <DataTable
          columns={columns}
          data={remotes}
          getRowId={(r) => r.name}
          selectable
          selectedIds={selectedIds}
          onSelectionChange={setSelectedIds}
          onRowClick={(r) => navigate(`/project/${project}/remotes/${r.name}`)}
          emptyMessage={t('remote.noRemotes')}
        />
      </div>

      <ConfirmModal
        open={confirmRemove}
        title={t('remote.removeTitle')}
        body={t('remote.removeConfirm', { count: selectedIds.size })}
        onConfirm={() => void handleRemove()}
        onCancel={() => setConfirmRemove(false)}
        confirmLabel={t('action.remove')}
        danger
      />

      {error != null && (
        <Modal open onClose={() => setError(null)} title={t('error.title')}>
          <p className="text-sm text-rose-600 dark:text-rose-400">{error}</p>
        </Modal>
      )}
    </section>
  );
}
