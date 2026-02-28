import { useState, useMemo, useCallback, useEffect } from 'react';
import { useNavigate, Link } from 'react-router';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { useInstances, useAllSharedServices, useRmBuildMutation, useArchiveMutation, qk } from '../api/hooks';
import { api } from '../api/endpoints';
import { projectName } from '../types/branded';
import { useRemovingProjects } from '../providers/RemovingProjectsProvider';
import type { InstanceSummary, ProjectSharedSummary, KnownProject } from '../types/api';
import type { ProjectName } from '../types/branded';
import { Trash, SpinnerGap, Archive, TrayArrowDown } from '@phosphor-icons/react';
import EmptyState from '../components/EmptyState';
import ConfirmModal from '../components/ConfirmModal';
import MassArchiveModal from '../components/MassArchiveModal';
import MassDeleteModal from '../components/MassDeleteModal';

interface ProjectGroup {
  readonly name: ProjectName;
  readonly root: string | null;
  readonly instances: readonly InstanceSummary[];
  readonly runningCount: number;
  readonly stoppedCount: number;
  readonly sharedTotal: number;
  readonly sharedRunning: number;
  readonly archived: boolean;
}

function buildProjectGroups(
  instances: readonly InstanceSummary[],
  sharedProjects: readonly ProjectSharedSummary[],
  knownProjects: readonly KnownProject[],
): readonly ProjectGroup[] {
  const map = new Map<string, { instances: InstanceSummary[]; root: string | null; archived: boolean }>();

  for (const kp of knownProjects) {
    if (!map.has(kp.name)) {
      map.set(kp.name, { instances: [], root: kp.project_root, archived: kp.archived === true });
    }
  }

  for (const inst of instances) {
    const key = inst.project as string;
    const entry = map.get(key);
    if (entry != null) {
      entry.instances.push(inst);
      if (entry.root == null && inst.project_root != null) {
        entry.root = inst.project_root;
      }
    } else {
      map.set(key, { instances: [inst], root: inst.project_root ?? null, archived: false });
    }
  }

  for (const sp of sharedProjects) {
    if (!map.has(sp.project)) {
      map.set(sp.project, { instances: [], root: null, archived: false });
    }
  }

  const sharedMap = new Map<string, ProjectSharedSummary>();
  for (const sp of sharedProjects) {
    sharedMap.set(sp.project, sp);
  }

  return Array.from(map.entries())
    .map(([name, { instances: insts, root, archived }]) => ({
      name: name as ProjectName,
      root,
      instances: insts,
      runningCount: insts.filter((i) => i.status === 'running' || i.status === 'checked_out').length,
      stoppedCount: insts.filter((i) => i.status === 'stopped').length,
      sharedTotal: sharedMap.get(name)?.total ?? 0,
      sharedRunning: sharedMap.get(name)?.running ?? 0,
      archived,
    }))
    .sort((a, b) => (a.name as string).localeCompare(b.name as string));
}

export default function ProjectsPage() {
  const { t } = useTranslation();
  const { data, isLoading, error } = useInstances();
  const { data: sharedData } = useAllSharedServices();
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  useEffect(() => {
    if (data == null) return;
    const byProject = new Map<string, typeof data.instances>();
    for (const inst of data.instances) {
      const key = inst.project as string;
      let arr = byProject.get(key);
      if (arr == null) { arr = []; byProject.set(key, arr); }
      arr.push(inst);
    }
    for (const [proj, instances] of byProject) {
      const kps = data.known_projects.filter((kp) => kp.name === proj);
      queryClient.setQueryData(qk.instances(projectName(proj)), { instances, known_projects: kps });
    }
  }, [data, queryClient]);

  useEffect(() => {
    if (sharedData == null) return;
    for (const ps of sharedData.projects) {
      if (ps.total > 0) {
        void queryClient.prefetchQuery({
          queryKey: qk.sharedServices(ps.project),
          queryFn: () => api.sharedLs(ps.project),
          staleTime: 10_000,
        });
      }
    }
  }, [sharedData, queryClient]);
  const rmBuildMut = useRmBuildMutation();
  const archiveMut = useArchiveMutation();
  const { removing } = useRemovingProjects();

  const [confirmProject, setConfirmProject] = useState<string | null>(null);
  const [archiveProject, setArchiveProject] = useState<string | null>(null);
  const [massArchiveOpen, setMassArchiveOpen] = useState(false);
  const [massArchiving, setMassArchiving] = useState(false);
  const [massDeleteOpen, setMassDeleteOpen] = useState(false);
  const [massDeleting, setMassDeleting] = useState(false);

  const allGroups = useMemo(
    () => buildProjectGroups(data?.instances ?? [], sharedData?.projects ?? [], data?.known_projects ?? []),
    [data, sharedData],
  );

  const groups = useMemo(() => allGroups.filter((g) => !g.archived), [allGroups]);
  const archivedCount = useMemo(() => allGroups.filter((g) => g.archived).length, [allGroups]);

  const canRemoveBuild = useCallback(
    (group: ProjectGroup) =>
      group.instances.length === 0 && group.sharedTotal === 0 && !removing.has(group.name as string),
    [removing],
  );

  const handleRemoveBuild = useCallback(() => {
    if (confirmProject == null) return;
    rmBuildMut.mutate(
      { project: confirmProject },
      {
        onSettled: () => setConfirmProject(null),
      },
    );
  }, [confirmProject, rmBuildMut]);

  const handleArchive = useCallback(() => {
    if (archiveProject == null) return;
    archiveMut.mutate(
      { project: archiveProject },
      {
        onSettled: () => setArchiveProject(null),
      },
    );
  }, [archiveProject, archiveMut]);

  const handleMassArchive = useCallback(
    async (projects: readonly string[]) => {
      setMassArchiving(true);
      try {
        for (const project of projects) {
          await archiveMut.mutateAsync({ project });
        }
      } finally {
        setMassArchiving(false);
        setMassArchiveOpen(false);
      }
    },
    [archiveMut],
  );

  const deletableGroups = useMemo(
    () => groups.filter((g) => canRemoveBuild(g)),
    [groups, canRemoveBuild],
  );

  const handleMassDelete = useCallback(
    async (projects: readonly string[]) => {
      setMassDeleting(true);
      try {
        for (const project of projects) {
          await rmBuildMut.mutateAsync({ project });
        }
      } finally {
        setMassDeleting(false);
        setMassDeleteOpen(false);
      }
    },
    [rmBuildMut],
  );

  if (isLoading) {
    return (
      <div className="page-shell">
        <p className="text-sm text-subtle-ui">{t('projects.loading')}</p>
      </div>
    );
  }

  if (error != null) {
    return (
      <div className="page-shell">
        <p className="text-sm text-rose-500">{t('projects.loadError', { error: String(error) })}</p>
      </div>
    );
  }

  return (
    <div className="page-shell">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-2xl font-bold text-main">{t('projects.title')}</h1>
        <div className="flex items-center gap-2">
          {deletableGroups.length > 1 && (
            <button
              onClick={() => setMassDeleteOpen(true)}
              className="btn btn-outline flex items-center gap-1.5 text-sm"
            >
              <Trash size={16} />
              {t('projects.bulkDelete')}
            </button>
          )}
          {groups.length > 1 && (
            <button
              onClick={() => setMassArchiveOpen(true)}
              className="btn btn-outline flex items-center gap-1.5 text-sm"
            >
              <TrayArrowDown size={16} />
              {t('projects.bulkArchive')}
            </button>
          )}
        </div>
      </div>
      {groups.length === 0 ? (
        <EmptyState message={t('projects.empty')} />
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {groups.map((group) => {
            const isRemoving = removing.has(group.name as string);
            return (
              <div
                key={group.name}
                onClick={() => void navigate(`/project/${group.name}`)}
                className={`glass-panel group cursor-pointer p-5 transition-all hover:-translate-y-0.5 hover:border-blue-400/50 dark:hover:border-blue-300/50 ${isRemoving ? 'opacity-60' : ''}`}
              >
                <div className="flex items-start justify-between">
                  <h2 className="text-base font-semibold text-main group-hover:text-blue-600 dark:group-hover:text-blue-300 transition-colors">
                    {group.name}
                  </h2>
                  {isRemoving ? (
                    <span className="flex items-center gap-1.5 text-xs text-rose-500 shrink-0 ml-2">
                      <SpinnerGap size={14} className="animate-spin" />
                      {t('projects.removing')}
                    </span>
                  ) : (
                    <span className="flex items-center gap-0.5 shrink-0 ml-2">
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          setArchiveProject(group.name as string);
                        }}
                        className="p-1 rounded cursor-pointer text-subtle-ui hover:text-amber-600 hover:bg-amber-50 focus-visible:text-amber-600 focus-visible:bg-amber-50 dark:hover:text-amber-300 dark:hover:bg-amber-900/30 dark:focus-visible:text-amber-300 dark:focus-visible:bg-amber-900/30 transition-colors"
                        title={t('projects.archive')}
                      >
                        <Archive size={16} />
                      </button>
                      {canRemoveBuild(group) && (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            setConfirmProject(group.name as string);
                          }}
                          className="p-1 rounded cursor-pointer text-subtle-ui hover:text-rose-600 hover:bg-rose-50 focus-visible:text-rose-600 focus-visible:bg-rose-50 dark:hover:text-rose-300 dark:hover:bg-rose-900/30 dark:focus-visible:text-rose-300 dark:focus-visible:bg-rose-900/30 transition-colors"
                          title={t('projects.removeBuild')}
                        >
                          <Trash size={16} />
                        </button>
                      )}
                    </span>
                  )}
                </div>
                <div className="mt-3 text-xs text-muted-ui space-y-1">
                  {isRemoving ? (
                    <div className="flex items-center gap-1.5 text-rose-500">
                      <span className="h-1.5 w-1.5 rounded-full bg-rose-500 animate-pulse" />
                      {t('projects.removingBuild')}
                    </div>
                  ) : (
                    <>
                      <div className="flex items-center gap-3">
                        {group.instances.length > 0 ? (
                          <>
                            <span>{t('projects.instanceCount', { count: group.instances.length })}</span>
                            {group.runningCount > 0 && (
                              <span className="flex items-center gap-1">
                                <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                                {t('projects.runningCount', { count: group.runningCount })}
                              </span>
                            )}
                            {group.stoppedCount > 0 && (
                              <span className="flex items-center gap-1">
                                <span className="h-1.5 w-1.5 rounded-full bg-rose-500" />
                                {t('projects.stoppedCount', { count: group.stoppedCount })}
                              </span>
                            )}
                          </>
                        ) : (
                          <span className="text-subtle-ui">{t('projects.noCoasts')}</span>
                        )}
                      </div>
                      {group.sharedTotal > 0 && (
                        <div className="flex items-center gap-3">
                          <span>{t('projects.sharedCount', { count: group.sharedTotal })}</span>
                          {group.sharedRunning > 0 && (
                            <span className="flex items-center gap-1">
                              <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                              {t('projects.runningCount', { count: group.sharedRunning })}
                            </span>
                          )}
                        </div>
                      )}
                    </>
                  )}
                </div>
                {group.root != null && !isRemoving && (
                  <p className="mt-2 text-xs font-mono text-subtle-ui truncate">
                    {group.root}
                  </p>
                )}
              </div>
            );
          })}
        </div>
      )}

      {archivedCount > 0 && (
        <div className="mt-6 text-center">
          <Link
            to="/archived"
            className="text-sm text-blue-600 hover:text-blue-700 dark:text-blue-400 dark:hover:text-blue-300 transition-colors"
          >
            {t('projects.archivedLink', { count: archivedCount })}
          </Link>
        </div>
      )}

      <ConfirmModal
        open={confirmProject != null}
        title={t('projects.removeBuildConfirmTitle')}
        body={t('projects.removeBuildConfirmBody', { project: confirmProject ?? '' })}
        onConfirm={handleRemoveBuild}
        onCancel={() => setConfirmProject(null)}
        confirmLabel={t('projects.removeBuild')}
        danger
      />

      <ConfirmModal
        open={archiveProject != null}
        title={t('projects.archiveConfirmTitle')}
        body={t('projects.archiveConfirmBody', { project: archiveProject ?? '' })}
        onConfirm={handleArchive}
        onCancel={() => setArchiveProject(null)}
        confirmLabel={t('projects.archive')}
        danger
      />

      <MassArchiveModal
        open={massArchiveOpen}
        projects={groups.map((g) => ({
          name: g.name as string,
          runningCount: g.runningCount,
          stoppedCount: g.stoppedCount,
          sharedTotal: g.sharedTotal,
        }))}
        onArchive={handleMassArchive}
        onClose={() => setMassArchiveOpen(false)}
        archiving={massArchiving}
      />

      <MassDeleteModal
        open={massDeleteOpen}
        projects={deletableGroups.map((g) => ({
          name: g.name as string,
          root: g.root,
        }))}
        onDelete={(ps) => void handleMassDelete(ps)}
        onClose={() => setMassDeleteOpen(false)}
        deleting={massDeleting}
      />
    </div>
  );
}
