import { useState, useMemo, useCallback } from 'react';
import { useParams, Link } from 'react-router';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { ArrowSquareOut, Warning } from '@phosphor-icons/react';
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
  useImages,
  useVolumes,
  useExecSessions,
  useMcpServers,
  usePortHealth,
} from '../api/hooks';
import { api } from '../api/endpoints';
import Breadcrumb from '../components/Breadcrumb';
import StatusBadge from '../components/StatusBadge';
import RemoteBadge from '../components/RemoteBadge';
import HealthDot from '../components/HealthDot';
import TabBar, { type TabDef } from '../components/TabBar';
import Modal from '../components/Modal';
import ConfirmModal from '../components/ConfirmModal';
import AssignModal from '../components/AssignModal';
import { ApiError } from '../api/client';

import InstanceExecTab from './InstanceExecTab';
import InstancePortsTab from './InstancePortsTab';
import InstanceServicesTab from './InstanceServicesTab';
import InstanceLogsTab from './InstanceLogsTab';
import InstanceStatsTab from './InstanceStatsTab';
import InstanceImagesTab from './InstanceImagesTab';
import InstanceSecretsTab from './InstanceSecretsTab';
import InstanceVolumesTab from './InstanceVolumesTab';
import InstanceFilesTab from './InstanceFilesTab';
import InstanceMcpTab from './InstanceMcpTab';

type TabId = 'exec' | 'files' | 'ports' | 'services' | 'logs' | 'secrets' | 'mcp' | 'stats' | 'images' | 'volumes';
const VALID_TABS = new Set<string>(['exec', 'files', 'ports', 'services', 'logs', 'secrets', 'mcp', 'stats', 'images', 'volumes']);

function parseTab(raw: string | undefined): TabId {
  if (raw != null && VALID_TABS.has(raw)) return raw as TabId;
  return 'exec';
}

export default function InstanceDetailPage() {
  const { t, i18n } = useTranslation();
  const params = useParams<{ project: string; name: string; tab: string }>();
  const project = projectName(params.project ?? '');
  const name = instanceName(params.name ?? '');
  const activeTab = parseTab(params.tab);

  const queryClient = useQueryClient();
  const { data } = useInstances(project);
  const { data: gitInfo } = useProjectGit(project);
  const instances = data?.instances ?? [];
  const instance: InstanceSummary | undefined = instances.find(
    (i) => (i.name as string) === (name as string),
  );

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

  const { data: execData } = useExecSessions(project, name, isRunning);
  const { data: portsData } = usePorts(project, name);
  const { data: healthData } = usePortHealth(project as string, name as string);
  const { data: servicesData } = useServices(project, name);
  const { data: secretsData } = useSecrets(project, name);
  const { data: imagesData } = useImages(project, name);
  const { data: volumesData } = useVolumes(project, name);
  const { data: mcpData } = useMcpServers(project as string, name as string);

  const execCount = execData?.length ?? 0;
  const portsCount = portsData?.ports?.length ?? 0;
  const servicesCount = servicesData?.services?.length ?? 0;
  const secretsCount = secretsData?.length ?? 0;
  const imagesCount = imagesData?.length ?? 0;
  const volumesCount = volumesData?.length ?? 0;
  const mcpCount = mcpData?.servers?.length ?? 0;
  const downServices = useMemo(
    () => servicesData?.services?.filter((s) => s.status !== 'running') ?? [],
    [servicesData],
  );

  const basePath = `/instance/${project}/${name}`;
  const tabs: readonly TabDef<TabId>[] = useMemo(
    () => [
      { id: 'exec' as const, label: `${t('tab.exec')}${execCount > 0 ? ` (${execCount})` : ''}`, to: `${basePath}/exec` },
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
    [basePath, t, i18n.language, execCount, portsCount, servicesCount, secretsCount, mcpCount, imagesCount, volumesCount, downServices.length],
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
              </>
            ) : !isProvisioning ? (
              <ActionBtn
                label={t('action.start')}
                variant="primary"
                className="!h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                onClick={() => void act(() => startMut.mutateAsync({ name, project }))}
              />
            ) : null}
            {instance.checked_out ? (
              <ActionBtn
                label={t('action.uncheckout')}
                variant="outline"
                className="!h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                onClick={() => void act(() => checkoutMut.mutateAsync({ project }))}
              />
            ) : isRunning && portsCount > 0 ? (
              <ActionBtn
                label={t('action.checkout')}
                variant="primary"
                className="!h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                onClick={() => void act(() => checkoutMut.mutateAsync({ project, name }))}
              />
            ) : isRunning && portsCount === 0 ? (
              <span className="inline-flex items-center h-8 px-3 rounded text-[12px] font-medium bg-[var(--surface-strong)] text-subtle-ui">
                {t('instance.noPorts')}
              </span>
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
          {/* Header */}
          <div className="flex items-center gap-3 mb-2">
            <h1 className="text-2xl font-bold text-main">{name}</h1>
            {instance.remote_name != null && <RemoteBadge remoteName={instance.remote_name} />}
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
            <StatusBadge status={instance.status} />
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
                {activeTab === 'exec' && <InstanceExecTab project={project} name={name} />}
                {activeTab === 'files' && <InstanceFilesTab project={project} name={name} />}
                {activeTab === 'ports' && <InstancePortsTab project={project} name={name} checkedOut={instance.checked_out} />}
                {activeTab === 'services' && <InstanceServicesTab project={project} name={name} checkedOut={instance.checked_out} />}
                {activeTab === 'logs' && <InstanceLogsTab project={project} name={name} />}
                {activeTab === 'stats' && <InstanceStatsTab project={project} name={name} />}
                {activeTab === 'images' && <InstanceImagesTab project={project} name={name} />}
                {activeTab === 'secrets' && (
                  <InstanceSecretsTab
                    project={project}
                    name={name}
                    buildId={instance.build_id ?? null}
                  />
                )}
                {activeTab === 'mcp' && <InstanceMcpTab project={project as string} name={name as string} />}
                {activeTab === 'volumes' && <InstanceVolumesTab project={project} name={name} />}
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
