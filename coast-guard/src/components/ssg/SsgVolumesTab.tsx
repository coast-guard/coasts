import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router';
import type { SsgVolumeInfo } from '../../types/api';
import { useSsgVolumes } from '../../api/hooks';
import DataTable, { type Column } from '../DataTable';

interface SsgVolumesTabProps {
    readonly project: string;
}

/**
 * Visually identical to {@link InstanceVolumesTab} but scoped to
 * the SSG outer DinD's inner Docker daemon (i.e. `docker volume
 * ls` from inside the per-project SSG container). Rows are
 * clickable and navigate to
 * `/project/<p>/ssg/local/volumes/<volumeName>`, where the
 * dedicated SSG volume detail page renders the full `docker
 * volume inspect` view.
 */
export default function SsgVolumesTab({ project }: SsgVolumesTabProps) {
    const { t, i18n } = useTranslation();
    const { data, isLoading, error } = useSsgVolumes(project);

    const volumes = data?.volumes ?? [];
    const basePath = `/project/${project}/ssg/local`;

    const columns: readonly Column<SsgVolumeInfo>[] = useMemo(
        () => [
            {
                key: 'name',
                header: t('volumes.name'),
                render: (r) => (
                    <Link
                        to={`${basePath}/volumes/${encodeURIComponent(r.name)}`}
                        className="font-medium text-[var(--primary)] hover:underline"
                    >
                        <span className="font-mono text-xs">{r.name}</span>
                    </Link>
                ),
            },
            {
                key: 'driver',
                header: t('volumes.driver'),
                render: (r) => <span className="text-xs">{r.driver}</span>,
            },
            {
                key: 'mountpoint',
                header: t('volumes.mountpoint'),
                render: (r) => (
                    <span
                        className="font-mono text-xs text-subtle-ui truncate max-w-[300px] inline-block"
                        title={r.mountpoint}
                    >
                        {r.mountpoint}
                    </span>
                ),
            },
            {
                key: 'scope',
                header: t('volumes.scope'),
                render: (r) => (
                    <span className="inline-block px-2 py-0.5 rounded-full text-[10px] font-semibold bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-500/20">
                        {r.scope}
                    </span>
                ),
            },
        ],
        [t, i18n.language, basePath],
    );

    if (isLoading) return <p className="text-sm text-subtle-ui py-4">{t('volumes.loading')}</p>;
    if (error != null) {
        return (
            <p className="text-sm text-rose-500 py-4">
                {t('volumes.loadError', { error: String(error) })}
            </p>
        );
    }

    return (
        <div className="glass-panel overflow-hidden">
            <DataTable
                columns={columns}
                data={volumes as SsgVolumeInfo[]}
                getRowId={(r) => r.name}
                onRowClick={(r) => {
                    window.location.hash = `${basePath}/volumes/${encodeURIComponent(r.name)}`;
                }}
                emptyMessage={t('ssg.noVolumes')}
            />
        </div>
    );
}
