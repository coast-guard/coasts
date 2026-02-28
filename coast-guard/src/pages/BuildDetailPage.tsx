import { useParams } from 'react-router';
import { useTranslation } from 'react-i18next';
import {
  useBuildsInspect,
  useBuildsDockerImages,
  useBuildsImages,
  useBuildsCompose,
  useBuildsCoastfile,
} from '../api/hooks';
import Breadcrumb from '../components/Breadcrumb';
import BuildTabContent from '../components/BuildTabContent';

export default function BuildDetailPage() {
  const { t } = useTranslation();
  const { project, buildId } = useParams<{ project: string; buildId: string }>();

  const { data: inspect } = useBuildsInspect(project ?? '', buildId);
  const { data: dockerImages } = useBuildsDockerImages(project ?? '', buildId);
  const { data: cachedImages } = useBuildsImages(project ?? '', buildId);
  const { data: compose } = useBuildsCompose(project ?? '', buildId);
  const { data: coastfile } = useBuildsCoastfile(project ?? '', buildId);

  const crumbs = [
    { label: t('nav.projects'), to: '/' },
    { label: project ?? '', to: `/project/${project}` },
    { label: t('projectTab.builds'), to: `/project/${project}/builds` },
    { label: buildId ?? 'latest' },
  ];

  return (
    <div className="page-shell">
      <Breadcrumb items={crumbs} />
      <h2 className="text-lg font-semibold text-main mb-4">
        {t('build.buildId')}: <span className="font-mono">{buildId}</span>
      </h2>
      <BuildTabContent
        project={project ?? ''}
        inspect={inspect ?? null}
        dockerImages={dockerImages?.images ?? []}
        cachedImages={cachedImages?.images ?? []}
        cachedTotalBytes={cachedImages?.total_size_bytes ?? 0}
        coastfile={coastfile?.content ?? null}
        compose={compose?.content ?? null}
      />
    </div>
  );
}
