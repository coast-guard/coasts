import { useMemo } from 'react';
import { useNavigate, useParams } from 'react-router';
import { useTranslation } from 'react-i18next';
import Breadcrumb from '../components/Breadcrumb';
import TabBar, { type TabDef } from '../components/TabBar';
import { useSsgState } from '../api/hooks';
import SsgExecTab from '../components/ssg/SsgExecTab';
import SsgPortsTab from '../components/ssg/SsgPortsTab';
import SsgServicesTab from '../components/ssg/SsgServicesTab';
import SsgLogsTab from '../components/ssg/SsgLogsTab';
import SsgSecretsTab from '../components/ssg/SsgSecretsTab';
import SsgStatsTab from '../components/ssg/SsgStatsTab';
import SsgImagesTab from '../components/ssg/SsgImagesTab';
import SsgVolumesTab from '../components/ssg/SsgVolumesTab';

type SsgTab =
    | 'exec'
    | 'ports'
    | 'services'
    | 'logs'
    | 'secrets'
    | 'stats'
    | 'images'
    | 'volumes';

const VALID_TABS = new Set<SsgTab>([
    'exec',
    'ports',
    'services',
    'logs',
    'secrets',
    'stats',
    'images',
    'volumes',
]);

function isValidTab(s: string | undefined): s is SsgTab {
    return s != null && (VALID_TABS as Set<string>).has(s);
}

export default function SsgLocalPage() {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const { project, tab: rawTab } = useParams<{ project: string; tab?: string }>();
    const project_ = project ?? '';
    const activeTab: SsgTab = isValidTab(rawTab) ? rawTab : 'exec';
    const { data: state } = useSsgState(project_);

    const basePath = `/project/${project_}/ssg/local`;
    const tabs: readonly TabDef<SsgTab>[] = useMemo(
        () => [
            { id: 'exec', label: t('ssg.tab.exec'), to: `${basePath}/exec` },
            { id: 'ports', label: t('ssg.tab.ports'), to: `${basePath}/ports` },
            {
                id: 'services',
                label: t('ssg.tab.services'),
                to: `${basePath}/services`,
            },
            { id: 'logs', label: t('ssg.tab.logs'), to: `${basePath}/logs` },
            {
                id: 'secrets',
                label: t('ssg.tab.secrets'),
                to: `${basePath}/secrets`,
            },
            { id: 'stats', label: t('ssg.tab.stats'), to: `${basePath}/stats` },
            {
                id: 'images',
                label: t('ssg.tab.images'),
                to: `${basePath}/images`,
            },
            {
                id: 'volumes',
                label: t('ssg.tab.volumes'),
                to: `${basePath}/volumes`,
            },
        ],
        [basePath, t],
    );

    const noBuildYet =
        state != null &&
        state.latest_build_id == null &&
        state.services.length === 0;

    return (
        <div className="page-shell">
            <Breadcrumb
                className="flex items-center gap-1.5 text-sm text-muted-ui mb-4"
                items={[
                    { label: t('nav.projects'), to: '/' },
                    { label: project_, to: `/project/${project_}` },
                    // Trailing tab is intentionally omitted: the
                    // tab nav below is already the source-of-truth
                    // for the active tab, and the breadcrumb stays
                    // visually identical across all 8 sub-tabs.
                    { label: t('build.ssgBuilds') },
                ]}
            />

            {noBuildYet ? (
                <section className="glass-panel p-6 text-sm text-subtle-ui">
                    {t('ssg.notBuiltYet')}
                </section>
            ) : (
                <>
                    <StatusBanner project={project_} />
                    <TabBar tabs={tabs} active={activeTab} />
                    <div className="mt-1">
                        {activeTab === 'exec' && <SsgExecTab project={project_} />}
                        {activeTab === 'ports' && <SsgPortsTab project={project_} />}
                        {activeTab === 'services' && (
                            <SsgServicesTab project={project_} />
                        )}
                        {activeTab === 'logs' && <SsgLogsTab project={project_} />}
                        {activeTab === 'secrets' && (
                            <SsgSecretsTab project={project_} />
                        )}
                        {activeTab === 'stats' && <SsgStatsTab project={project_} />}
                        {activeTab === 'images' && <SsgImagesTab project={project_} />}
                        {activeTab === 'volumes' && (
                            <SsgVolumesTab project={project_} />
                        )}
                    </div>
                </>
            )}

            {/* Hidden anchor for navigate() callbacks (used by status
                  banner's pinned/latest links). */}
            <span hidden onClick={() => navigate('#')} />
        </div>
    );
}

function StatusBanner({ project }: { readonly project: string }) {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const { data } = useSsgState(project);

    if (data == null) {
        return null;
    }

    return (
        <div className="glass-panel p-4 mb-4">
            <div className="flex items-center gap-3 flex-wrap">
                <h2 className="text-base font-semibold text-main">
                    {t('ssg.statusHeader')}
                </h2>
                <StatusPill status={data.status} t={t} />
                {data.latest_build_id != null && (
                    <button
                        type="button"
                        className="font-mono text-xs text-[var(--primary)] hover:text-[var(--primary-strong)] hover:underline transition-colors cursor-pointer bg-transparent border-0 p-0"
                        onClick={() =>
                            navigate(
                                `/project/${project}/ssg-builds/${encodeURIComponent(
                                    data.latest_build_id ?? '',
                                )}`,
                            )
                        }
                    >
                        <span className="text-subtle-ui mr-1.5">
                            {t('build.ssgLatestBadge')}:
                        </span>
                        {data.latest_build_id}
                    </button>
                )}
                {data.pinned_build_id != null && (
                    <button
                        type="button"
                        className="font-mono text-xs text-[var(--primary)] hover:text-[var(--primary-strong)] hover:underline transition-colors cursor-pointer bg-transparent border-0 p-0"
                        onClick={() =>
                            navigate(
                                `/project/${project}/ssg-builds/${encodeURIComponent(
                                    data.pinned_build_id ?? '',
                                )}`,
                            )
                        }
                    >
                        <span className="text-subtle-ui mr-1.5">
                            {t('build.ssgPinnedBadge')}:
                        </span>
                        {data.pinned_build_id}
                    </button>
                )}
            </div>
            {data.message && (
                <p className="mt-2 text-xs text-subtle-ui">{data.message}</p>
            )}
        </div>
    );
}

function StatusPill({
    status,
    t,
}: {
    readonly status: string | null;
    readonly t: ReturnType<typeof useTranslation>['t'];
}) {
    if (status == null) {
        return <span className="text-subtle-ui text-xs">{t('ssg.statusAbsent')}</span>;
    }
    const color =
        status === 'running'
            ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
            : status === 'stopped'
                ? 'bg-rose-500/10 text-rose-600 dark:text-rose-400'
                : 'bg-amber-500/10 text-amber-600 dark:text-amber-400';
    return (
        <span
            className={`px-1.5 py-0.5 rounded font-mono text-[10px] uppercase ${color}`}
        >
            {status}
        </span>
    );
}
