import { useMemo, useState, useCallback } from 'react';
import { Link } from 'react-router';
import { useTranslation } from 'react-i18next';
import { useInstances, useUnarchiveMutation } from '../api/hooks';
import type { KnownProject } from '../types/api';
import { ArrowCounterClockwise, TrayArrowUp } from '@phosphor-icons/react';
import EmptyState from '../components/EmptyState';
import MassUnarchiveModal from '../components/MassUnarchiveModal';

export default function ArchivedProjectsPage() {
  const { t } = useTranslation();
  const { data, isLoading } = useInstances();
  const unarchiveMut = useUnarchiveMutation();
  const [pendingUnarchive, setPendingUnarchive] = useState<string | null>(null);
  const [massUnarchiveOpen, setMassUnarchiveOpen] = useState(false);
  const [massUnarchiving, setMassUnarchiving] = useState(false);

  const archivedProjects = useMemo<readonly KnownProject[]>(
    () => (data?.known_projects ?? []).filter((kp) => kp.archived === true),
    [data],
  );

  const handleUnarchive = useCallback(
    (project: string) => {
      setPendingUnarchive(project);
      unarchiveMut.mutate(
        { project },
        { onSettled: () => setPendingUnarchive(null) },
      );
    },
    [unarchiveMut],
  );

  const handleMassUnarchive = useCallback(
    async (projects: readonly string[]) => {
      setMassUnarchiving(true);
      try {
        for (const project of projects) {
          await unarchiveMut.mutateAsync({ project });
        }
      } finally {
        setMassUnarchiving(false);
        setMassUnarchiveOpen(false);
      }
    },
    [unarchiveMut],
  );

  if (isLoading) {
    return (
      <div className="page-shell">
        <p className="text-sm text-subtle-ui">{t('projects.loading')}</p>
      </div>
    );
  }

  return (
    <div className="page-shell">
      <div className="mb-6 flex items-center gap-3">
        <Link
          to="/"
          className="text-sm text-blue-600 hover:text-blue-700 dark:text-blue-400 dark:hover:text-blue-300 transition-colors"
        >
          &larr; {t('projects.backToProjects')}
        </Link>
        {archivedProjects.length > 1 && (
          <button
            onClick={() => setMassUnarchiveOpen(true)}
            className="btn btn-outline flex items-center gap-1.5 text-sm ml-auto"
          >
            <TrayArrowUp size={16} />
            {t('projects.bulkUnarchive')}
          </button>
        )}
      </div>

      <h1 className="text-2xl font-bold mb-6 text-main">{t('projects.archivedTitle')}</h1>

      {archivedProjects.length === 0 ? (
        <EmptyState message={t('projects.archivedEmpty')} />
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {archivedProjects.map((kp) => {
            const isUnarchiving = pendingUnarchive === kp.name;
            return (
              <div
                key={kp.name}
                className={`glass-panel p-5 transition-all ${isUnarchiving ? 'opacity-60' : ''}`}
              >
                <div className="flex items-start justify-between">
                  <h2 className="text-base font-semibold text-main">{kp.name}</h2>
                  <button
                    onClick={() => handleUnarchive(kp.name)}
                    disabled={isUnarchiving}
                    className="p-1 rounded cursor-pointer text-subtle-ui hover:text-blue-600 hover:bg-blue-50 focus-visible:text-blue-600 focus-visible:bg-blue-50 dark:hover:text-blue-300 dark:hover:bg-blue-900/30 dark:focus-visible:text-blue-300 dark:focus-visible:bg-blue-900/30 transition-colors shrink-0 ml-2 disabled:opacity-50 disabled:cursor-not-allowed"
                    title={t('projects.unarchive')}
                  >
                    <ArrowCounterClockwise size={16} />
                  </button>
                </div>
                {kp.project_root != null && (
                  <p className="mt-2 text-xs font-mono text-subtle-ui truncate">
                    {kp.project_root}
                  </p>
                )}
              </div>
            );
          })}
        </div>
      )}

      <MassUnarchiveModal
        open={massUnarchiveOpen}
        projects={archivedProjects.map((kp) => ({
          name: kp.name,
          project_root: kp.project_root,
        }))}
        onUnarchive={(ps) => void handleMassUnarchive(ps)}
        onClose={() => setMassUnarchiveOpen(false)}
        unarchiving={massUnarchiving}
      />
    </div>
  );
}
