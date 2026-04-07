import { useState, useMemo, useCallback } from 'react';
import { useParams, Link } from 'react-router';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { ArrowSquareOut, Warning, CloudArrowUp } from '@phosphor-icons/react';
import { projectName, instanceName } from '../types/branded';
import type { InstanceSummary } from '../types/api';
import {
  useInstances,
  useProjectGit,
  useStopMutation,
  useStartMutation,
  useRestartServicesMutation,
  useCheckoutMutation,
  usePorts,
  useServices,
  useSecrets,
  useMcpServers,
  usePortHealth,
  useRemotesLs,
  useImages,
  useVolumes,
} from '../api/hooks';
import { api } from '../api/endpoints';
import Breadcrumb from '../components/Breadcrumb';
import StatusBadge from '../components/StatusBadge';
import HealthDot from '../components/HealthDot';
import TabBar, { type TabDef } from '../components/TabBar';
import Modal from '../components/Modal';
import ConfirmModal from '../components/ConfirmModal';
import AssignModal from '../components/AssignModal';
import { ApiError } from '../api/client';

import { buildLocalExecTerminalConfig } from '../hooks/useTerminalSessions';
import PersistentTerminal from '../components/PersistentTerminal';
import InstancePortsTab from './InstancePortsTab';
import InstanceServicesTab from './InstanceServicesTab';
import InstanceLogsTab from './InstanceLogsTab';
import InstanceStatsTab from './InstanceStatsTab';
import InstanceFilesTab from './InstanceFilesTab';
import InstanceSecretsTab from './InstanceSecretsTab';
import InstanceMcpTab from './InstanceMcpTab';
import InstanceImagesTab from './InstanceImagesTab';
import InstanceVolumesTab from './InstanceVolumesTab';

type TabId = 'exec' | 'local-exec' | 'files' | 'ports' | 'services' | 'logs' | 'secrets' | 'mcp' | 'stats' | 'images' | 'volumes';
const VALID_TABS = new Set<string>(['exec', 'local-exec', 'files', 'ports', 'services', 'logs', 'secrets', 'mcp', 'stats', 'images', 'volumes']);

function parseTab(raw: string | undefined): TabId {
  if (raw != null && VALID_TABS.has(raw)) return raw as TabId;
  return 'exec';
}

export default function RemoteInstanceDetailPage() {
  const { t, i18n } = useTranslation();
  const params = useParams<{ project: string; name: string; tab: string }>();
  const project = projectName(params.project ?? '');
  const name = instanceName(params.name ?? '');
  const activeTab = parseTab(params.tab);

  const queryClient = useQueryClient();
  const { data } = useInstances(project);
  const { data: gitInfo } = useProjectGit(project);
  const { data: remotesData } = useRemotesLs();
  const instances = data?.instances ?? [];
  const instance: InstanceSummary | undefined = instances.find(
    (i) => (i.name as string) === (name as string),
  );

  const remoteHost = instance?.remote_host;
  const remoteName = useMemo(() => {
    if (!remoteHost) return null;
    const allRemotes = remotesData?.remotes ?? [];
    return allRemotes.find(
      (r: { name: string; host: string }) => r.name === remoteHost || r.host === remoteHost,
    )?.name ?? remoteHost;
  }, [remoteHost, remotesData]);

  const occupiedWorktrees = useMemo(
    () => new Set(instances.filter((i) => i.worktree != null).map((i) => i.worktree as string)),
    [instances],
  );

  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [assignOpen, setAssignOpen] = useState(false);
  const [confirmRestart, setConfirmRestart] = useState(false);
  const stopMut = useStopMutation();
  const startMut = useStartMutation();
  const restartServicesMut = useRestartServicesMutation();
  const checkoutMut = useCheckoutMutation();

  const act = useCallback(
    async (fn: () => Promise<unknown>) => {
      try {
        await fn();
      } catch (e) {
        setErrorMsg(e instanceof ApiError ? e.body.error : String(e));
      }
    },
    [],
  );

  const handleAssign = useCallback(async (worktree: string) => {
    const result = await api.assignInstance(project as string, name as string, worktree);
    void queryClient.invalidateQueries({ queryKey: ['instances'] });
    if (result.error) {
      setErrorMsg(result.error.error);
    }
  }, [project, name, queryClient]);

  const handleUnassign = useCallback(async () => {
    const result = await api.unassignInstance(project as string, name as string);
    void queryClient.invalidateQueries({ queryKey: ['instances'] });
    if (result.error) {
      setErrorMsg(result.error.error);
    }
  }, [project, name, queryClient]);

  const isRunning = instance != null && (instance.status === 'running' || instance.status === 'checked_out');
  const isProvisioning = instance != null && (instance.status === 'enqueued' || instance.status === 'provisioning' || instance.status === 'assigning');
  const canAssign = instance != null && (instance.status === 'running' || instance.status === 'checked_out' || instance.status === 'idle');
  const isAssigned = instance?.worktree != null;
  const isTransitioning = instance != null && (instance.status === 'assigning' || instance.status === 'unassigning');

  const { data: portsData } = usePorts(project, name);
  const { data: healthData } = usePortHealth(project as string, name as string);
  const { data: servicesData, error: servicesError, isLoading: servicesLoading } = useServices(project, name);
  const { data: secretsData } = useSecrets(project, name);
  const { data: mcpData } = useMcpServers(project as string, name as string);
  const { data: imagesData } = useImages(project, name);
  const { data: volumesData } = useVolumes(project, name);
  const remoteReachable = remoteHost ? !servicesError : true;

  const portsCount = portsData?.ports?.length ?? 0;
  const servicesCount = servicesData?.services?.length ?? 0;
  const secretsCount = secretsData?.length ?? 0;
  const mcpCount = mcpData?.servers?.length ?? 0;
  const imagesCount = imagesData?.length ?? 0;
  const volumesCount = volumesData?.length ?? 0;
  const remoteWarmingUp = remoteHost != null && remoteReachable && !servicesLoading && servicesCount === 0 && instance?.status === 'running';
  const downServices = useMemo(
    () => servicesData?.services?.filter((s) => !s.status.startsWith('running')) ?? [],
    [servicesData],
  );

  const basePath = `/remote-instance/${project}/${name}`;
  const tabs: readonly TabDef<TabId>[] = useMemo(
    () => [
      { id: 'exec' as const, label: t('tab.exec'), to: `${basePath}/exec` },
      { id: 'local-exec' as const, label: t('tab.localExec'), to: `${basePath}/local-exec` },
      { id: 'files' as const, label: t('tab.files'), to: `${basePath}/files` },
      { id: 'ports' as const, label: `${t('tab.ports')}${portsCount > 0 ? ` (${portsCount})` : ''}`, to: `${basePath}/ports` },
      { id: 'services' as const, label: `${t('tab.services')}${servicesCount > 0 ? ` (${servicesCount})` : ''}`, to: `${basePath}/services`, warn: downServices.length > 0 },
      { id: 'logs' as const, label: t('tab.logs'), to: `${basePath}/logs` },
      { id: 'secrets' as const, label: `${t('tab.secrets')}${secretsCount > 0 ? ` (${secretsCount})` : ''}`, to: `${basePath}/secrets` },
      { id: 'mcp' as const, label: `${t('tab.mcp')}${mcpCount > 0 ? ` (${mcpCount})` : ''}`, to: `${basePath}/mcp` },
      { id: 'stats' as const, label: t('tab.stats'), to: `${basePath}/stats` },
      { id: 'images' as const, label: `${t('tab.images')}${imagesCount > 0 ? ` (${imagesCount})` : ''}`, to: `${basePath}/images` },
      { id: 'volumes' as const, label: `${t('tab.volumes')}${volumesCount > 0 ? ` (${volumesCount})` : ''}`, to: `${basePath}/volumes` },
    ],
    [basePath, t, i18n.language, portsCount, servicesCount, secretsCount, mcpCount, imagesCount, volumesCount, downServices.length],
  );

  return (
    <div className="page-shell">
      <div className="flex items-start justify-between mb-4">
        <Breadcrumb
          className="flex items-center gap-1.5 text-sm text-muted-ui"
          items={[
            { label: t('nav.projects'), to: '/' },
            { label: project, to: `/project/${project}` },
            { label: name },
          ]}
        />
        {instance != null && (
          <div className="flex items-center gap-2">
            {isRunning ? (
              <>
                <ActionBtn
                  label={t('action.restartServices')}
                  variant="outline"
                  className="!h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                  onClick={() => setConfirmRestart(true)}
                />
                <ActionBtn
                  label={t('action.stop')}
                  variant="outline"
                  className="!h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                  onClick={() => void act(() => stopMut.mutateAsync({ name, project }))}
                />
                {instance.checked_out ? (
                  <ActionBtn
                    label={t('action.uncheckout')}
                    variant="outline"
                    className="!h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                    onClick={() => void act(() => checkoutMut.mutateAsync({ project }))}
                  />
                ) : portsCount > 0 ? (
                  <ActionBtn
                    label={t('action.checkout')}
                    variant="primary"
                    className="!h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                    onClick={() => void act(() => checkoutMut.mutateAsync({ project, name }))}
                  />
                ) : null}
              </>
            ) : !isProvisioning ? (
              <ActionBtn
                label={t('action.start')}
                variant="primary"
                className="!h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                onClick={() => void act(() => startMut.mutateAsync({ name, project }))}
              />
            ) : null}
          </div>
        )}
      </div>

      {instance == null ? (
        <div className="glass-panel py-12 text-center text-subtle-ui">
          <p>{t('instance.notFound', { name })}</p>
        </div>
      ) : (
        <>
          <div className="flex items-center gap-3 mb-2">
            <h1 className="text-2xl font-bold text-main">{name}</h1>
            {remoteName && (
              <span className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 text-xs font-medium rounded-full shrink-0 ${
                remoteReachable
                  ? 'bg-indigo-500/12 border border-indigo-500/30 text-indigo-700 dark:text-indigo-300'
                  : 'bg-red-500/12 border border-red-500/30 text-red-700 dark:text-red-300'
              }`}>
                <CloudArrowUp size={12} weight="fill" />
                {remoteName}
              </span>
            )}
            {instance.primary_port_url != null && (
              <a
                href={instance.primary_port_url}
                target="_blank"
                rel="noopener noreferrer"
                className="inline-flex items-center gap-1.5 px-2.5 py-0.5 text-xs font-medium rounded-full bg-[var(--primary)]/12 border border-[var(--primary)]/30 text-[var(--primary-strong)] dark:text-[var(--primary)] hover:bg-[var(--primary)]/20 transition-colors shrink-0"
              >
                <HealthDot healthy={healthData?.ports?.find((p) => p.logical_name === (instance.primary_port_service ?? 'web'))?.healthy} size={6} />
                {instance.primary_port_service ?? 'web'}
                <ArrowSquareOut size={11} />
              </a>
            )}
            <StatusBadge status={instance.status} remoteDown={!remoteReachable} warmingUp={remoteWarmingUp} />
          </div>

          <div className="flex items-center gap-3 text-sm font-mono text-subtle-ui mb-4">
            <span>
              {instance.worktree != null ? (
                <>
                  <span className="text-fuchsia-600 dark:text-fuchsia-400">{instance.worktree}</span>
                  {instance.branch != null && instance.branch !== instance.worktree && (
                    <span className="ml-2 text-subtle-ui opacity-60">({instance.branch})</span>
                  )}
                </>
              ) : (
                <span>{instance.branch ?? t('instance.noBranch')}</span>
              )}
            </span>

            {canAssign && !isTransitioning && (
              <div className="flex items-center gap-1.5 ml-1">
                {!isAssigned ? (
                  <button
                    className="btn btn-primary !h-6 !px-2.5 !py-0 !text-[11px] !font-medium !rounded"
                    onClick={() => setAssignOpen(true)}
                  >
                    {t('action.assign')}
                  </button>
                ) : (
                  <>
                    <button
                      className="btn btn-outline !h-6 !px-2.5 !py-0 !text-[11px] !font-medium !rounded"
                      onClick={() => setAssignOpen(true)}
                    >
                      {t('action.reassign')}
                    </button>
                    <button
                      className="btn btn-outline !h-6 !px-2.5 !py-0 !text-[11px] !font-medium !rounded text-orange-600 dark:text-orange-400 border-orange-300 dark:border-orange-500/40 hover:bg-orange-50 dark:hover:bg-orange-500/10"
                      onClick={() => void handleUnassign().catch((err) => setErrorMsg(String(err)))}
                    >
                      {t('action.unassign')}
                    </button>
                  </>
                )}
              </div>
            )}

            {isTransitioning && (
              <span className="text-xs text-subtle-ui animate-pulse">
                {instance.status === 'assigning' ? t('status.assigning') : t('status.unassigning')}
              </span>
            )}
          </div>

          {instance.build_id != null && (
            <div className="flex items-center gap-2 text-sm mb-4">
              <span className="text-subtle-ui">{t('col.build')}:</span>
              <Link
                to={`/project/${project}/builds/${encodeURIComponent(instance.build_id)}`}
                className="font-mono text-xs text-[var(--primary)] hover:text-[var(--primary-strong)] hover:underline"
              >
                {instance.build_id}
              </Link>
            </div>
          )}

          {isRunning && downServices.length > 0 && (
            <Link
              to={`${basePath}/services`}
              className="inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-lg bg-amber-500/10 border border-amber-500/30 text-amber-700 dark:text-amber-300 hover:bg-amber-500/20 transition-colors mb-4"
            >
              <Warning size={14} weight="fill" />
              {downServices.length} service{downServices.length !== 1 ? 's' : ''} down
            </Link>
          )}

          {!isRunning ? (
            <div className="glass-panel py-12 text-center text-subtle-ui">
              <p>{isProvisioning ? t(instance?.status === 'assigning' ? 'instance.assigning' : instance?.status === 'enqueued' ? 'instance.enqueued' : 'instance.provisioning') : t('instance.notRunning')}</p>
            </div>
          ) : (
            <>
              <TabBar tabs={tabs} active={activeTab} />
              <div className="mt-1">
                {activeTab === 'exec' && remoteName && <RemoteExecTab remoteName={remoteName} project={project as string} instanceName={name as string} />}
                {activeTab === 'local-exec' && <LocalExecTab project={project} name={name} />}
                {activeTab === 'files' && <InstanceFilesTab project={project} name={name} />}
                {activeTab === 'ports' && <InstancePortsTab project={project} name={name} checkedOut={instance.checked_out} />}
                {activeTab === 'services' && <InstanceServicesTab project={project} name={name} checkedOut={instance.checked_out} basePath={basePath} />}
                {activeTab === 'logs' && <InstanceLogsTab project={project} name={name} />}
                {activeTab === 'secrets' && (
                  <InstanceSecretsTab
                    project={project}
                    name={name}
                    buildId={instance.build_id ?? null}
                  />
                )}
                {activeTab === 'mcp' && <InstanceMcpTab project={project as string} name={name as string} />}
                {activeTab === 'stats' && <InstanceStatsTab project={project} name={name} />}
                {activeTab === 'images' && <InstanceImagesTab project={project} name={name} basePath={basePath} />}
                {activeTab === 'volumes' && <InstanceVolumesTab project={project} name={name} basePath={basePath} />}
              </div>
            </>
          )}
        </>
      )}

      <AssignModal
        open={assignOpen}
        instanceName={name as string}
        worktrees={gitInfo?.worktrees ?? []}
        occupiedWorktrees={occupiedWorktrees}
        onAssign={(wt) => {
          setAssignOpen(false);
          void handleAssign(wt).catch((err) => setErrorMsg(String(err)));
        }}
        onClose={() => setAssignOpen(false)}
      />

      <ConfirmModal
        open={confirmRestart}
        title={t('instance.restartServicesTitle')}
        body={t('instance.restartServicesBody', { name })}
        confirmLabel={t('action.restartServices')}
        danger
        onConfirm={() => {
          setConfirmRestart(false);
          void act(() => restartServicesMut.mutateAsync({ name, project }));
        }}
        onCancel={() => setConfirmRestart(false)}
      />

      <Modal open={errorMsg != null} title={t('error.title')} onClose={() => setErrorMsg(null)}>
        <p className="text-rose-600 dark:text-rose-400">{errorMsg}</p>
      </Modal>
    </div>
  );
}

function RemoteExecTab({ remoteName, project, instanceName }: { readonly remoteName: string; readonly project: string; readonly instanceName: string }) {
  const config = useMemo(
    () => {
      const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      const host = window.location.host;
      const en = encodeURIComponent(remoteName);
      const containerName = `${project}-coasts-${instanceName}`;
      const cmd = encodeURIComponent(`docker exec -it ${containerName} sh`);
      const sc = encodeURIComponent(`remote-instance-exec:${project}:${instanceName}`);
      return {
        listSessionsUrl: `/api/v1/remote/exec/sessions?name=${en}&scope=${sc}`,
        deleteSessionUrl: (id: string) => `/api/v1/remote/exec/sessions?id=${encodeURIComponent(id)}`,
        wsUrl: (sid: string | null) => {
          let url = `${proto}//${host}/api/v1/remote/exec/interactive?name=${en}&scope=${sc}&command=${cmd}`;
          if (sid != null) url += `&session_id=${encodeURIComponent(sid)}`;
          return url;
        },
        uploadUrl: null,
        uploadMeta: null,
        configKey: `remote-instance-exec:${project}:${instanceName}`,
      };
    },
    [remoteName, project, instanceName],
  );
  return (
    <div className="flex flex-col h-full">
      <div className="flex-1 min-h-0">
        <PersistentTerminal config={config} />
      </div>
    </div>
  );
}

function LocalExecTab({ project, name }: { readonly project: string; readonly name: string }) {
  const config = useMemo(
    () => buildLocalExecTerminalConfig(project, name),
    [project, name],
  );
  return (
    <div className="flex flex-col h-full">
      <div className="flex-1 min-h-0">
        <PersistentTerminal config={config} />
      </div>
    </div>
  );
}

function ActionBtn({
  label,
  variant,
  className,
  onClick,
}: {
  readonly label: string;
  readonly variant: 'primary' | 'outline';
  readonly className?: string;
  readonly onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`btn ${
        variant === 'primary'
          ? 'btn-primary'
          : 'btn-outline'
      } ${className ?? ''}`}
    >
      {label}
    </button>
  );
}
