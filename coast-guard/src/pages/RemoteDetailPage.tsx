import { useMemo } from 'react';
import { useParams } from 'react-router';
import { useTranslation } from 'react-i18next';
import { useRemotesLs, useRemoteStats } from '../api/hooks';
import { buildRemoteExecTerminalConfig } from '../hooks/useTerminalSessions';
import Breadcrumb from '../components/Breadcrumb';
import TabBar, { type TabDef } from '../components/TabBar';
import PersistentTerminal from '../components/PersistentTerminal';
import RemoteStatsTab from './RemoteStatsTab';
import RemoteLogsTab from './RemoteLogsTab';

function formatSize(bytes: number): string {
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  if (bytes >= 1_048_576) return `${Math.round(bytes / 1_048_576)} MB`;
  return `${Math.round(bytes / 1024)} KB`;
}

type RemoteTab = 'shell' | 'logs' | 'stats' | 'info';
const VALID_TABS = new Set<string>(['shell', 'logs', 'stats', 'info']);

function parseTab(raw: string | undefined): RemoteTab {
  if (raw != null && VALID_TABS.has(raw)) return raw as RemoteTab;
  return 'shell';
}

export default function RemoteDetailPage() {
  const { t } = useTranslation();
  const { project, remote: remoteName, tab: rawTab } = useParams<{
    project: string;
    remote: string;
    tab: string;
  }>();
  const activeTab = parseTab(rawTab);
  const name = remoteName ?? '';

  const { data: remotesData } = useRemotesLs();
  const { data: statsData } = useRemoteStats();
  const entry = remotesData?.remotes?.find((r) => r.name === name);
  const stats = statsData?.stats?.[name];

  const termConfig = useMemo(
    () => buildRemoteExecTerminalConfig(name),
    [name],
  );

  const basePath = `/project/${project}/remotes/${name}`;

  const tabs: readonly TabDef<RemoteTab>[] = useMemo(
    () => [
      { id: 'shell' as const, label: t('remote.shellTab'), to: `${basePath}/shell` },
      { id: 'logs' as const, label: t('remote.logsTab'), to: `${basePath}/logs` },
      { id: 'stats' as const, label: t('remote.statsTab'), to: `${basePath}/stats` },
      { id: 'info' as const, label: t('remote.infoTab'), to: `${basePath}/info` },
    ],
    [basePath, t],
  );

  return (
    <div className="page-shell">
      <div className="flex items-start justify-between mb-4 min-h-[32px]">
        <Breadcrumb
          className="flex items-center gap-1.5 text-sm text-muted-ui"
          items={[
            { label: t('nav.projects'), to: '/' },
            { label: project ?? '', to: `/project/${project}` },
            { label: t('projectTab.remotes'), to: `/project/${project}/remotes` },
            { label: name },
          ]}
        />
      </div>

      <h1 className="text-2xl font-bold text-main">{name}</h1>
      {entry && (
        <p className="mt-1 text-sm text-subtle-ui font-mono">
          {entry.user}@{entry.host}:{entry.port}
        </p>
      )}
      {stats && (
        <p className="mt-1 mb-4 text-xs text-subtle-ui flex items-center gap-3">
          <span>{formatSize(stats.used_memory_bytes)} / {formatSize(stats.total_memory_bytes)} RAM</span>
          <span className="text-[var(--border)]">|</span>
          <span>{stats.cpu_count} CPU @ {stats.cpu_usage_percent.toFixed(0)}%</span>
          <span className="text-[var(--border)]">|</span>
          <span>{formatSize(stats.used_disk_bytes)} / {formatSize(stats.total_disk_bytes)} disk</span>
          {stats.service_version && (
            <>
              <span className="text-[var(--border)]">|</span>
              <span>coast-service v{stats.service_version}</span>
            </>
          )}
        </p>
      )}
      {!stats && entry && <div className="mb-4" />}

      <TabBar tabs={tabs} active={activeTab} />

      <div className="mt-1">
        {activeTab === 'shell' && (
          <PersistentTerminal config={termConfig} />
        )}

        {activeTab === 'logs' && (
          <RemoteLogsTab remoteName={name} />
        )}

        {activeTab === 'stats' && (
          <RemoteStatsTab remoteName={name} />
        )}

        {activeTab === 'info' && entry && (
          <section className="mt-1">
            <div className="glass-panel p-5 space-y-3">
              <InfoRow label={t('remote.nameLabel')} value={entry.name} mono />
              <InfoRow label={t('remote.infoUser')} value={entry.user} mono />
              <InfoRow label={t('remote.infoHost')} value={entry.host} mono />
              <InfoRow label={t('remote.infoPort')} value={String(entry.port)} mono />
              <InfoRow label={t('remote.infoSshKey')} value={entry.ssh_key ?? '—'} mono />
              <InfoRow label={t('remote.infoSync')} value={entry.sync_strategy} />
              <InfoRow label={t('remote.infoAdded')} value={entry.created_at} />
            </div>
          </section>
        )}

        {activeTab === 'info' && !entry && (
          <div className="glass-panel p-6 text-sm text-subtle-ui mt-1">
            Remote not found.
          </div>
        )}
      </div>
    </div>
  );
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-baseline gap-4">
      <span className="text-xs font-medium text-subtle-ui uppercase tracking-wider w-32 shrink-0">
        {label}
      </span>
      <span className={`text-sm text-main ${mono ? 'font-mono' : ''}`}>
        {value}
      </span>
    </div>
  );
}
