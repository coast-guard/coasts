import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router';
import type { SsgImageInfo } from '../../types/api';
import { useSsgImages } from '../../api/hooks';
import DataTable, { type Column } from '../DataTable';

interface SsgImagesTabProps {
    readonly project: string;
}

function truncateId(id: string): string {
    const sha = id.startsWith('sha256:') ? id.slice(7) : id;
    return sha.slice(0, 12);
}

/**
 * Visually identical to {@link InstanceImagesTab} but scoped to
 * the SSG outer DinD's inner Docker daemon (i.e. `docker image
 * ls` from inside the per-project SSG container). Rows are
 * clickable and navigate to
 * `/project/<p>/ssg/local/images/<imageId>`, where the shared
 * {@link ImageDetailPage} renders the full `docker inspect` view
 * powered by {@link useImageInspect} (which routes to the SSG
 * inspect endpoint when the URL begins with `/project/...`).
 */
export default function SsgImagesTab({ project }: SsgImagesTabProps) {
    const { t, i18n } = useTranslation();
    const { data, isLoading, error } = useSsgImages(project);

    const images = data?.images ?? [];
    const basePath = `/project/${project}/ssg/local`;

    const columns: readonly Column<SsgImageInfo>[] = useMemo(
        () => [
            {
                key: 'repository',
                header: t('images.repository'),
                render: (r) => (
                    <Link
                        to={`${basePath}/images/${encodeURIComponent(r.id)}`}
                        className="font-medium text-[var(--primary)] hover:underline"
                    >
                        {r.repository}
                    </Link>
                ),
            },
            {
                key: 'tag',
                header: t('images.tag'),
                render: (r) => (
                    <span className="inline-block px-2 py-0.5 rounded-full text-[10px] font-semibold bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-500/20">
                        {r.tag}
                    </span>
                ),
            },
            {
                key: 'id',
                header: t('images.id'),
                render: (r) => (
                    <span className="font-mono text-xs text-subtle-ui" title={r.id}>
                        {truncateId(r.id)}
                    </span>
                ),
            },
            {
                key: 'created',
                header: t('images.created'),
                render: (r) => <span className="text-xs text-subtle-ui">{r.created}</span>,
            },
            {
                key: 'size',
                header: t('images.size'),
                render: (r) => <span className="text-xs font-mono">{r.size}</span>,
            },
        ],
        [t, i18n.language, basePath],
    );

    if (isLoading) return <p className="text-sm text-subtle-ui py-4">{t('images.loading')}</p>;
    if (error != null) {
        return (
            <p className="text-sm text-rose-500 py-4">
                {t('images.loadError', { error: String(error) })}
            </p>
        );
    }

    return (
        <div className="glass-panel overflow-hidden">
            <DataTable
                columns={columns}
                data={images as SsgImageInfo[]}
                getRowId={(r) => r.id}
                onRowClick={(r) => {
                    window.location.hash = `${basePath}/images/${encodeURIComponent(r.id)}`;
                }}
                emptyMessage={t('ssg.noImages')}
            />
        </div>
    );
}
