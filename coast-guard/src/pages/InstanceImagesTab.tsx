import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router';
import type { ProjectName, InstanceName } from '../types/branded';
import type { ImageSummary } from '../types/api';
import { useImages } from '../api/hooks';
import DataTable, { type Column } from '../components/DataTable';

interface Props {
  readonly project: ProjectName;
  readonly name: InstanceName;
}

function truncateId(id: string): string {
  const sha = id.startsWith('sha256:') ? id.slice(7) : id;
  return sha.slice(0, 12);
}

export default function InstanceImagesTab({ project, name }: Props) {
  const { t, i18n } = useTranslation();
  const { data, isLoading, error } = useImages(project, name);

  const images = data ?? [];

  const columns: readonly Column<ImageSummary>[] = useMemo(
    () => [
      {
        key: 'repository',
        header: t('images.repository'),
        render: (r) => (
          <Link
            to={`/instance/${project}/${name}/images/${encodeURIComponent(r.id)}`}
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
    [t, i18n.language, project, name],
  );

  if (isLoading) return <p className="text-sm text-subtle-ui py-4">{t('images.loading')}</p>;
  if (error != null) return <p className="text-sm text-rose-500 py-4">{t('images.loadError', { error: String(error) })}</p>;

  return (
    <div className="glass-panel overflow-hidden">
      <DataTable
        columns={columns}
        data={images as ImageSummary[]}
        getRowId={(r) => r.id}
        onRowClick={(r) => {
          window.location.hash = `/instance/${project}/${name}/images/${encodeURIComponent(r.id)}`;
        }}
        emptyMessage={t('images.empty')}
      />
    </div>
  );
}
