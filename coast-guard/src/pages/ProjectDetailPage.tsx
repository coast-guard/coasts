import { useState, useCallback, useMemo, useEffect, useRef } from 'react';
import { useParams, useNavigate, Link } from 'react-router';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { projectName, instanceName } from '../types/branded';
import type { InstanceSummary } from '../types/api';
import {
  useInstances,
  useProjectGit,
  useSharedServices,
  useStopMutation,
  useStartMutation,
  useRmMutation,
  useCheckoutMutation,
  useBuildsLs,
} from '../api/hooks';
import { api } from '../api/endpoints';
import { buildHostTerminalConfig } from '../hooks/useTerminalSessions';
import Breadcrumb from '../components/Breadcrumb';
import TabBar, { type TabDef } from '../components/TabBar';
import DataTable, { type Column } from '../components/DataTable';
import Toolbar, { type ToolbarAction } from '../components/Toolbar';
import ConfirmModal from '../components/ConfirmModal';
import AssignModal from '../components/AssignModal';
import CreateCoastModal from '../components/CreateCoastModal';
import StatusBadge from '../components/StatusBadge';
import RemoteBadge from '../components/RemoteBadge';
import PersistentTerminal from '../components/PersistentTerminal';
import SharedServicesPanel from '../components/SharedServicesPanel';
import Modal from '../components/Modal';
import PrimaryPortHealthDot from '../components/PrimaryPortHealthDot';
import { ApiError } from '../api/client';
import { ArrowRight, ArrowSquareOut, SpinnerGap, Warning } from '@phosphor-icons/react';
import { useRemovingProjects } from '../providers/RemovingProjectsProvider';
import { useProjectMemory } from '../hooks/useProjectMemory';
import { formatBytes } from '../lib/formatBytes';
import BuildModal from '../components/BuildModal';
import DotLoader from '../components/DotLoader';
import BuildsListPanel from '../components/BuildsListPanel';

interface PendingOp {
  readonly type: 'assign' | 'unassign' | 'provision-assign';
  readonly targetWorktree: string;
}

type ProjectTab = 'coasts' | 'shared-services' | 'builds' | 'terminal';
const VALID_PROJECT_TABS = new Set<string>(['coasts', 'shared-services', 'builds', 'terminal']);

function parseProjectTab(raw: string | undefined): ProjectTab {
  if (raw != null && VALID_PROJECT_TABS.has(raw)) return raw as ProjectTab;
  return 'coasts';
}

export default function ProjectDetailPage() {
  const { t, i18n } = useTranslation();
  const { project: rawProject, tab: rawTab } = useParams<{ project: string; tab: string }>();
  const project = projectName(rawProject ?? '');
  const activeTab = parseProjectTab(rawTab);
  const navigate = useNavigate();

  const { data, isLoading } = useInstances(project);
  const { data: gitInfo, isLoading: gitLoading } = useProjectGit(project);
  const { data: sharedData } = useSharedServices(project as string);
  const { data: buildsLsData } = useBuildsLs(project as string);
  const instances = data?.instances ?? [];
  const { removing } = useRemovingProjects();
  const isProjectRemoving = removing.has(rawProject ?? '');

  const { memoryMap, totalMemory } = useProjectMemory(
    project as string,
    instances,
    '/api/v1/stats/stream',
    'name',
  );

  const queryClient = useQueryClient();

  const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(new Set());
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const [assignTarget, setAssignTarget] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [buildModalOpen, setBuildModalOpen] = useState(false);

  const existingNames = useMemo(
    () => new Set(instances.map((i) => i.name as string)),
    [instances],
  );

  const [pendingOps, setPendingOps] = useState<Record<string, PendingOp>>({});
  const pendingOpsRef = useRef(pendingOps);
  pendingOpsRef.current = pendingOps;

  useEffect(() => {
    const cur = pendingOpsRef.current;
    const next: Record<string, PendingOp> = {};
    let changed = false;
    for (const [name, op] of Object.entries(cur)) {
      const inst = instances.find((i) => (i.name as string) === name);
      if (op.type === 'provision-assign') {
        if (inst && (inst.status === 'enqueued' || inst.status === 'provisioning' || inst.status === 'assigning')) {
          next[name] = op;
        } else {
          changed = true;
        }
      } else if (inst && (inst.status === 'assigning' || inst.status === 'unassigning')) {
        next[name] = op;
      } else {
        changed = true;
      }
    }
    if (changed) setPendingOps(next);
  }, [instances]);

  const occupiedWorktrees = useMemo(
    () => new Set(instances.filter((i) => i.worktree != null).map((i) => i.worktree as string)),
    [instances],
  );

  const stopMut = useStopMutation();
  const startMut = useStartMutation();
  const rmMut = useRmMutation();
  const checkoutMut = useCheckoutMutation();

  const handleUnassign = useCallback(async (name: string) => {
    setPendingOps((prev) => ({ ...prev, [name]: { type: 'unassign', targetWorktree: 'default' } }));
    const result = await api.unassignInstance(project, name);
    void queryClient.invalidateQueries({ queryKey: ['instances'] });
    if (result.error) {
      setPendingOps((prev) => { const { [name]: _removed, ...rest } = prev; void _removed; return rest; });
      setErrorMsg(result.error.error);
    }
  }, [project, queryClient]);

  const handleAssign = useCallback(async (name: string, worktree: string) => {
    setPendingOps((prev) => ({ ...prev, [name]: { type: 'assign', targetWorktree: worktree } }));
    const result = await api.assignInstance(project, name, worktree);
    void queryClient.invalidateQueries({ queryKey: ['instances'] });
    if (result.error) {
      setPendingOps((prev) => { const { [name]: _removed, ...rest } = prev; void _removed; return rest; });
      setErrorMsg(result.error.error);
    }
  }, [project, queryClient]);

  const selectedNames = useMemo(
    () => instances.filter((i) => selectedIds.has(i.name as string)).map((i) => i.name),
    [instances, selectedIds],
  );

  const batchAction = useCallback(
    async (action: (vars: { name: typeof instanceName extends (s: string) => infer R ? R : never; project: typeof project }) => Promise<unknown>) => {
      const errors: string[] = [];
      for (const name of selectedNames) {
        try {
          await action({ name: instanceName(name), project });
        } catch (e) {
          errors.push(`${name}: ${e instanceof ApiError ? e.body.error : String(e)}`);
        }
      }
      setSelectedIds(new Set());
      if (errors.length > 0) setErrorMsg(errors.join('\n'));
    },
    [selectedNames, project],
  );

  const toolbarActions: readonly ToolbarAction[] = useMemo(
    () => [
      { label: t('action.stop'), variant: 'outline' as const, onClick: () => void batchAction((v) => stopMut.mutateAsync(v)) },
      { label: t('action.start'), variant: 'outline' as const, onClick: () => void batchAction((v) => startMut.mutateAsync(v)) },
      { label: t('action.remove'), variant: 'danger' as const, onClick: () => setConfirmRemove(true) },
    ],
    [batchAction, stopMut, startMut, t, i18n.language],
  );

  const columns: readonly Column<InstanceSummary>[] = useMemo(
    () => [
      {
        key: 'name',
        header: t('col.name'),
        className: 'w-80',
        headerClassName: 'w-80',
        render: (r) => (
          <div className="flex items-center gap-2 min-w-0">
            <span className="font-semibold truncate">{r.name}</span>
            {r.remote_name != null && <RemoteBadge remoteName={r.remote_name} />}
            {r.checked_out ? (
              <span className="inline-flex items-center gap-1 text-[11px] font-semibold text-[var(--primary)]">
                <ArrowRight size={12} />
                {t('instance.checkedOut')}
              </span>
            ) : ['running', 'idle'].includes(r.status) ? (
              (r.port_count ?? 0) > 0 ? (
                <button
                  type="button"
                  className="btn btn-outline !px-2 !py-0.5 !text-[11px]"
                  onClick={(e) => {
                    e.stopPropagation();
                    void checkoutMut
                      .mutateAsync({ project, name: instanceName(r.name) })
                      .catch((err) => setErrorMsg(err instanceof ApiError ? err.body.error : String(err)));
                  }}
                >
                  {t('action.checkout')}
                </button>
              ) : (
                <span className="inline-block px-1.5 py-0.5 rounded text-[10px] font-medium bg-[var(--surface-strong)] text-subtle-ui">
                  {t('instance.noPorts')}
                </span>
              )
            ) : null}
            {r.primary_port_url != null && ['running', 'idle', 'checked_out'].includes(r.status) && (
              <a
                href={r.primary_port_url}
                target="_blank"
                rel="noopener noreferrer"
                onClick={(e) => e.stopPropagation()}
                className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[11px] font-medium border border-[var(--primary)]/30 bg-[var(--primary)]/10 text-[var(--primary)] hover:bg-[var(--primary)]/20 transition-colors shrink-0"
              >
                <PrimaryPortHealthDot project={project} name={r.name} service={r.primary_port_service} size={6} />
                {r.primary_port_service}
                <ArrowSquareOut size={11} />
              </a>
            )}
          </div>
        ),
      },
      {
        key: 'status',
        header: t('col.status'),
        className: 'w-40',
        headerClassName: 'w-40',
        render: (r) => {
          const mem = memoryMap.get(r.name as string);
          const isActive = ['running', 'checked_out', 'idle'].includes(r.status);
          const hasDown = isActive && r.down_service_count > 0;
          return (
            <div className="flex items-center gap-2">
              {hasDown ? (
                <span className="inline-flex items-center gap-1.5 px-2.5 py-0.5 text-xs font-medium rounded-full bg-amber-500/12 border border-amber-500/30 text-amber-700 dark:text-amber-300">
                  <Warning size={12} weight="fill" />
                  {r.down_service_count} down
                </span>
              ) : (
                <StatusBadge status={isActive ? 'running' : r.status} />
              )}
              {mem != null && isActive && (
                <span className="text-[11px] text-muted-ui">{formatBytes(mem.memoryUsed)}</span>
              )}
            </div>
          );
        },
      },
      {
        key: 'branch',
        header: t('col.branch'),
        className: 'w-52',
        headerClassName: 'w-52',
        render: (r) => {
          const pending = pendingOps[r.name as string];
          const isTransitioning = r.status === 'assigning' || r.status === 'unassigning';
          const isProvisioningWithWorktree = (r.status === 'enqueued' || r.status === 'provisioning') && pending?.type === 'provision-assign';
          if ((isTransitioning || isProvisioningWithWorktree) && pending) {
            return (
              <div className="flex items-center gap-1.5 font-mono text-xs">
                <span className={pending.type === 'unassign' ? 'text-subtle-ui' : 'text-fuchsia-600 dark:text-fuchsia-300'}>
                  {pending.targetWorktree}
                </span>
                <DotLoader className={pending.type === 'unassign' ? 'text-subtle-ui' : 'text-fuchsia-500'} />
              </div>
            );
          }
          if (isTransitioning) {
            return (
              <div className="flex items-center gap-1.5 font-mono text-xs">
                <span className="text-subtle-ui">{r.branch ?? '\u2014'}</span>
                <DotLoader className={r.status === 'assigning' ? 'text-fuchsia-500' : 'text-subtle-ui'} />
              </div>
            );
          }
          return <span className="font-mono text-xs">{r.branch ?? '\u2014'}</span>;
        },
      },
      {
        key: 'worktree',
        header: t('col.worktree'),
        className: 'w-56',
        headerClassName: 'w-56',
        render: (r) => {
          const pending = pendingOps[r.name as string];
          const isTransitioning = r.status === 'assigning' || r.status === 'unassigning';
          const isProvisioningWithWorktree = (r.status === 'enqueued' || r.status === 'provisioning') && pending?.type === 'provision-assign';

          if ((isTransitioning || isProvisioningWithWorktree) && pending) {
            return (
              <div className="flex items-center gap-1.5 font-mono text-xs">
                <span className={pending.type === 'unassign' ? 'text-subtle-ui' : 'text-fuchsia-600 dark:text-fuchsia-300'}>
                  {pending.targetWorktree}
                </span>
                <DotLoader className={pending.type === 'unassign' ? 'text-subtle-ui' : 'text-fuchsia-500'} />
              </div>
            );
          }

          if (isTransitioning) {
            return (
              <div className="flex items-center gap-1.5 font-mono text-xs">
                {r.worktree != null && <span className="text-subtle-ui">{r.worktree}</span>}
                <DotLoader className={r.status === 'assigning' ? 'text-fuchsia-500' : 'text-subtle-ui'} />
              </div>
            );
          }

          return (
            <div className="flex items-center gap-2">
              {r.worktree != null && (
                <>
                  <span className="font-mono text-xs">{r.worktree}</span>
                  {(r.status === 'running' || r.status === 'idle' || r.status === 'checked_out') && (
                    <button
                      type="button"
                      className="btn btn-outline !px-2 !py-0.5 !text-[11px]"
                      onClick={(e) => {
                        e.stopPropagation();
                        void handleUnassign(r.name as string).catch((err) =>
                          setErrorMsg(String(err)),
                        );
                      }}
                    >
                      {t('action.unassign')}
                    </button>
                  )}
                </>
              )}
              {r.worktree == null && (r.status === 'running' || r.status === 'idle' || r.status === 'checked_out') ? (
                <button
                  type="button"
                  className="btn btn-primary !px-2 !py-0.5 !text-[11px]"
                  onClick={(e) => {
                    e.stopPropagation();
                    setAssignTarget(r.name as string);
                  }}
                >
                  {t('action.assign')}
                </button>
              ) : r.worktree == null ? (
                <span className="font-mono text-xs">{'\u2014'}</span>
              ) : null}
            </div>
          );
        },
      },
      {
        key: 'build',
        header: t('col.build'),
        className: 'w-48',
        headerClassName: 'w-48',
        render: (r) => {
          const bid = r.build_id;
          if (!bid) return <span className="text-subtle-ui">{'\u2014'}</span>;
          return (
            <Link
              to={`/project/${project}/builds/${encodeURIComponent(bid)}`}
              className="font-mono text-xs text-[var(--primary)] hover:text-[var(--primary-strong)] hover:underline"
              onClick={(e) => e.stopPropagation()}
            >
              {bid}
            </Link>
          );
        },
      },
    ],
    [checkoutMut, handleUnassign, memoryMap, pendingOps, project, t, i18n.language],
  );

  const termConfig = useMemo(() => buildHostTerminalConfig(project), [project]);

  const basePath = `/project/${project}`;
  const sharedCount = sharedData?.services?.length ?? 0;
  const coastCount = instances.length;
  const buildsCount = buildsLsData?.builds?.length ?? 0;

  const tabs: readonly TabDef<ProjectTab>[] = useMemo(
    () => [
      { id: 'coasts' as const, label: `${t('projectTab.coasts')}${coastCount > 0 ? ` (${coastCount})` : ''}`, to: `${basePath}/coasts` },
      { id: 'shared-services' as const, label: `${t('projectTab.sharedServices')}${sharedCount > 0 ? ` (${sharedCount})` : ''}`, to: `${basePath}/shared-services` },
      { id: 'builds' as const, label: `${t('projectTab.builds')}${buildsCount > 0 ? ` (${buildsCount})` : ''}`, to: `${basePath}/builds` },
      { id: 'terminal' as const, label: t('projectTab.terminal'), to: `${basePath}/terminal` },
    ],
    [basePath, buildsCount, coastCount, sharedCount, t, i18n.language],
  );

  return (
    <div className="page-shell">
      <div className="flex items-start justify-between mb-4 min-h-[32px]">
        <Breadcrumb
          className="flex items-center gap-1.5 text-sm text-muted-ui"
          items={
            activeTab === 'coasts'
              ? [{ label: t('nav.projects'), to: '/' }, { label: project }]
              : [
                  { label: t('nav.projects'), to: '/' },
                  { label: project, to: `/project/${project}` },
                  { label: activeTab === 'shared-services' ? t('projectTab.sharedServices') : activeTab === 'builds' ? t('projectTab.builds') : t('projectTab.terminal') },
                ]
          }
        />
        {activeTab === 'coasts' ? (
          <button
            type="button"
            className="btn btn-primary !h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
            onClick={() => setCreateOpen(true)}
          >
            {t('create.button')}
          </button>
        ) : activeTab === 'builds' ? (
          <button
            type="button"
            className="btn btn-primary !h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
            onClick={() => setBuildModalOpen(true)}
          >
            {t('build.createNewBuild')}
          </button>
        ) : activeTab === 'shared-services' ? (
          <button
            type="button"
            className="btn btn-primary !h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
            onClick={() => {
              void api.sharedStartAll(project as string).then(() => {
                void queryClient.invalidateQueries({ queryKey: ['sharedServices'] });
                void queryClient.invalidateQueries({ queryKey: ['sharedServicesAll'] });
              });
            }}
          >
            {t('shared.refresh')}
          </button>
        ) : (
          <div className="h-8" />
        )}
      </div>
      <h1 className="text-2xl font-bold text-main">{project}</h1>
      <div className="mb-6 mt-2 flex items-center gap-3 text-sm">
        {gitLoading ? (
          <span className="text-subtle-ui">{t('project.branchLoading')}</span>
        ) : gitInfo?.is_git_repo === true ? (
          <span className="text-subtle-ui">
            {t('project.currentBranch')}{' '}
            <span className="font-semibold text-main">{gitInfo.current_branch ?? t('project.noBranch')}</span>
          </span>
        ) : (
          <span className="text-subtle-ui">{t('project.notGitRepo')}</span>
        )}
      </div>

      {isProjectRemoving && (
        <div className="mb-6 flex items-center gap-3 rounded-lg border border-rose-300 bg-rose-50 px-4 py-3 text-sm text-rose-700 dark:border-rose-700 dark:bg-rose-950/40 dark:text-rose-300">
          <SpinnerGap size={18} className="animate-spin shrink-0" />
          <span>{t('projects.removingBuildBanner')}</span>
        </div>
      )}

      <TabBar tabs={tabs} active={activeTab} />

      {activeTab === 'coasts' && (
        <section>
          <div className="glass-panel overflow-hidden">
            <Toolbar
              actions={toolbarActions}
              selectedCount={selectedNames.length}
              memorySummary={totalMemory > 0 ? t('toolbar.memory', { memory: formatBytes(totalMemory) }) : undefined}
            />
            {isLoading ? (
              <div className="p-6 text-sm text-subtle-ui">{t('project.loading')}</div>
            ) : (() => {
              const instanceTypes = Array.from(
                new Set(instances.map((i) => i.coastfile_type ?? 'default')),
              ).sort((a, b) => a.localeCompare(b));
              const orderedInstanceTypes = ['default', ...instanceTypes.filter((t) => t !== 'default')].filter(
                (t, i, arr) => arr.indexOf(t) === i && instanceTypes.includes(t),
              );
              const hasMultipleInstanceTypes = orderedInstanceTypes.length > 1;

              return !hasMultipleInstanceTypes ? (
                <DataTable
                  columns={columns}
                  data={instances}
                  getRowId={(r) => r.name as string}
                  selectable
                  selectedIds={selectedIds}
                  onSelectionChange={setSelectedIds}
                  onRowClick={(r) => void navigate(`/instance/${r.project}/${r.name}`)}
                  emptyMessage={t('project.emptyInstances', { project })}
                />
              ) : (
                <div className="p-4 space-y-4">
                  {orderedInstanceTypes.map((type) => {
                    const group = instances.filter((i) => (i.coastfile_type ?? 'default') === type);
                    if (group.length === 0) return null;
                    const groupIds = group.map((i) => i.name as string);
                    return (
                      <div key={type} className="rounded-lg border border-[var(--border)] overflow-hidden">
                        <div className="px-4 py-2 text-xs font-semibold uppercase tracking-wide text-subtle-ui bg-[var(--surface-muted)]">
                          {t('build.type')}: <span className="font-mono normal-case">{type}</span>
                        </div>
                        <DataTable
                          columns={columns}
                          data={group}
                          getRowId={(r) => r.name as string}
                          tableClassName="table-fixed"
                          selectable
                          selectedIds={selectedIds}
                          onSelectionChange={(next) => {
                            setSelectedIds((prev) => {
                              const nextSet = new Set(next);
                              const sectionOnly = [...nextSet].every((id) => groupIds.includes(id));
                              if (!sectionOnly) return nextSet;
                              const merged = new Set(prev);
                              const allBefore = groupIds.every((id) => prev.has(id));
                              if (nextSet.size === 0 && allBefore) {
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
                          onRowClick={(r) => void navigate(`/instance/${r.project}/${r.name}`)}
                          emptyMessage={t('project.emptyInstances', { project })}
                        />
                      </div>
                    );
                  })}
                </div>
              );
            })()}
          </div>
        </section>
      )}

      {activeTab === 'shared-services' && (
        <SharedServicesPanel project={project} />
      )}

      {activeTab === 'builds' && (
        <BuildsListPanel
          project={project as string}
          builds={buildsLsData?.builds ?? []}
          t={t}
          navigate={navigate}
        />
      )}

      {activeTab === 'terminal' && (
        <section>
          <PersistentTerminal config={termConfig} />
        </section>
      )}

      <ConfirmModal
        open={confirmRemove}
        title={t('project.removeTitle')}
        body={t('project.removeBody', { count: selectedNames.length })}
        onConfirm={() => {
          setConfirmRemove(false);
          void batchAction((v) => rmMut.mutateAsync(v));
        }}
        onCancel={() => setConfirmRemove(false)}
        confirmLabel={t('action.remove')}
        danger
      />

      <AssignModal
        open={assignTarget != null}
        instanceName={assignTarget ?? ''}
        worktrees={gitInfo?.worktrees ?? []}
        occupiedWorktrees={occupiedWorktrees}
        onAssign={(wt) => {
          const target = assignTarget;
          setAssignTarget(null);
          if (target) void handleAssign(target, wt).catch((err) => setErrorMsg(String(err)));
        }}
        onClose={() => setAssignTarget(null)}
      />

      <CreateCoastModal
        open={createOpen}
        project={project}
        existingNames={existingNames}
        builds={buildsLsData?.builds ?? []}
        worktrees={gitInfo?.worktrees ?? []}
        occupiedWorktrees={occupiedWorktrees}
        currentBranch={gitInfo?.current_branch}
        onError={setErrorMsg}
        onCreated={(name, worktree) => {
          setCreateOpen(false);
          if (worktree) {
            setPendingOps((prev) => ({
              ...prev,
              [name]: { type: 'provision-assign', targetWorktree: worktree },
            }));
          }
          void queryClient.invalidateQueries({ queryKey: ['instances'] });
        }}
        onClose={() => setCreateOpen(false)}
      />

      <Modal open={errorMsg != null} title={t('error.title')} onClose={() => setErrorMsg(null)}>
        <pre className="whitespace-pre-wrap text-sm font-mono text-rose-600 dark:text-rose-400">{errorMsg}</pre>
      </Modal>

      <BuildModal
        open={buildModalOpen}
        project={project as string}
        onClose={() => setBuildModalOpen(false)}
        onComplete={() => {
          setBuildModalOpen(false);
          void queryClient.invalidateQueries({ queryKey: ['buildsLs'] });
        }}
      />
    </div>
  );
}
