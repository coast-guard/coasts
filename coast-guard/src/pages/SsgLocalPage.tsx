import { useCallback, useMemo, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router';
import { useTranslation } from 'react-i18next';
import { ArrowClockwise } from '@phosphor-icons/react';
import Breadcrumb from '../components/Breadcrumb';
import TabBar, { type TabDef } from '../components/TabBar';
import SsgStatusBadge from '../components/SsgStatusBadge';
import ConfirmModal from '../components/ConfirmModal';
import Modal from '../components/Modal';
import { useSsgState } from '../api/hooks';
import { api } from '../api/endpoints';
import { ApiError } from '../api/client';
import { useQueryClient } from '@tanstack/react-query';
import { qk } from '../api/hooks';
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
            <div className="flex items-start justify-between mb-4">
                <Breadcrumb
                    className="flex items-center gap-1.5 text-sm text-muted-ui"
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
                {!noBuildYet && <SsgActionButtons project={project_} />}
            </div>

            {noBuildYet ? (
                <section className="glass-panel p-6 text-sm text-subtle-ui">
                    {t('ssg.notBuiltYet')}
                </section>
            ) : (
                <>
                    <SsgHeader project={project_} />
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

            {/* Hidden anchor used by header navigate() callbacks. */}
            <span hidden onClick={() => navigate('#')} />
        </div>
    );
}

/**
 * Top-right action cluster on the SSG Local page. Mirrors the
 * `InstanceDetailPage` button row exactly:
 *
 * 1. **Restart Services** (`btn-outline`, danger-confirm modal) —
 *    bounces every inner compose service inside the outer DinD
 *    via `docker compose -p <project>-ssg restart` (no service
 *    arg). Does NOT bounce the outer DinD itself.
 * 2. **Stop / Start** (`btn-outline`) — toggles the outer DinD
 *    container based on `data.status`. Calls `api.ssgStop` /
 *    `api.ssgStart`.
 * 3. **Checkout / Uncheckout** (`btn-primary` for checkout,
 *    `btn-outline` for uncheckout) — toggles canonical-port
 *    binding for ALL services. Maps to `coast ssg checkout --all`
 *    / `coast ssg uncheckout --all`. Hidden when the SSG isn't
 *    running.
 *
 * Toggle state for Checkout/Uncheckout: "checked out" means at
 * least one service has a canonical-port binding (`port.checked_out`
 * in `data.ports`). Same any-true rule the SSG ports tab uses.
 */
function SsgActionButtons({ project }: { readonly project: string }) {
    const { t } = useTranslation();
    const queryClient = useQueryClient();
    const { data } = useSsgState(project);

    const [confirmRestart, setConfirmRestart] = useState(false);
    const [opPending, setOpPending] = useState(false);
    const [errorMsg, setErrorMsg] = useState<string | null>(null);

    const isRunning = data?.status === 'running';
    const hasCheckout = (data?.ports ?? []).some((p) => p.checked_out);

    const invalidate = useCallback(() => {
        void queryClient.invalidateQueries({ queryKey: qk.ssgState(project) });
    }, [queryClient, project]);

    const act = useCallback(
        async (fn: () => Promise<unknown>) => {
            setOpPending(true);
            try {
                await fn();
                invalidate();
            } catch (e) {
                setErrorMsg(e instanceof ApiError ? e.body.error : String(e));
            } finally {
                setOpPending(false);
            }
        },
        [invalidate],
    );

    if (data == null) {
        return null;
    }

    return (
        <div className="flex items-center gap-2">
            {isRunning && (
                <>
                    <button
                        type="button"
                        className="btn btn-outline !h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                        disabled={opPending}
                        onClick={() => setConfirmRestart(true)}
                    >
                        {t('ssg.action.restartServices')}
                    </button>
                    <button
                        type="button"
                        className="btn btn-outline !h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                        disabled={opPending}
                        onClick={() => void act(() => api.ssgStop(project))}
                    >
                        {t('action.stop')}
                    </button>
                </>
            )}

            {!isRunning && (
                <button
                    type="button"
                    className="btn btn-primary !h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                    disabled={opPending}
                    onClick={() => void act(() => api.ssgStart(project))}
                >
                    {t('action.start')}
                </button>
            )}

            {isRunning &&
                (hasCheckout ? (
                    <button
                        type="button"
                        className="btn btn-outline !h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                        disabled={opPending}
                        onClick={() => void act(() => api.ssgUncheckoutAll(project))}
                    >
                        {t('action.uncheckout')}
                    </button>
                ) : (
                    <button
                        type="button"
                        className="btn btn-primary !h-8 !px-3.5 !py-1.5 !text-[14px] !font-semibold"
                        disabled={opPending}
                        onClick={() => void act(() => api.ssgCheckoutAll(project))}
                    >
                        {t('action.checkout')}
                    </button>
                ))}

            {opPending && (
                <span className="inline-flex items-center text-subtle-ui">
                    <ArrowClockwise size={16} className="animate-spin" />
                </span>
            )}

            <ConfirmModal
                open={confirmRestart}
                title={t('ssg.action.restartServicesTitle')}
                body={t('ssg.action.restartServicesBody', { project })}
                confirmLabel={t('ssg.action.restartServices')}
                danger
                onConfirm={() => {
                    setConfirmRestart(false);
                    void act(() => api.ssgRestartServices(project));
                }}
                onCancel={() => setConfirmRestart(false)}
            />

            <Modal
                open={errorMsg != null}
                title={t('error.title')}
                onClose={() => setErrorMsg(null)}
            >
                <p className="text-rose-600 dark:text-rose-400">{errorMsg}</p>
            </Modal>
        </div>
    );
}

/**
 * SSG-flavored header that mirrors the layout of
 * `InstanceDetailPage`: a single h1 + status pill row, followed
 * by a "Build:" line linking to the active SSG build artifact.
 *
 * The pre-Phase-33 `StatusBanner` wrapped everything in a
 * `glass-panel` rectangle, which made the SSG header visually
 * distinct from the rest of the project's detail pages. Switching
 * to the bare h1 + pill + build line keeps the SPA's instance and
 * SSG navs visually consistent.
 */
function SsgHeader({ project }: { readonly project: string }) {
    const { t } = useTranslation();
    const { data } = useSsgState(project);
    if (data == null) {
        return null;
    }

    const activeBuildId = data.pinned_build_id ?? data.latest_build_id;

    return (
        <>
            <div className="flex items-center gap-3 mb-2 flex-wrap">
                <h1 className="text-2xl font-bold text-main">
                    {t('ssg.detail.title')}
                </h1>
                <SsgStatusBadge status={data.status} />
            </div>

            {activeBuildId != null && (
                <div className="flex items-center gap-2 text-sm mb-4 flex-wrap">
                    <span className="text-subtle-ui">{t('col.build')}:</span>
                    <Link
                        to={`/project/${project}/ssg-builds/${encodeURIComponent(activeBuildId)}`}
                        className="font-mono text-xs text-[var(--primary)] hover:text-[var(--primary-strong)] hover:underline"
                    >
                        {activeBuildId}
                    </Link>
                    {data.pinned_build_id != null && (
                        <span className="inline-block px-2 py-0.5 rounded-full text-[10px] font-semibold bg-[var(--primary)]/10 text-[var(--primary-strong)] dark:text-[var(--primary)] border border-[var(--primary)]/20">
                            {t('build.ssgPinnedBadge')}
                        </span>
                    )}
                    {data.pinned_build_id == null && data.latest_build_id != null && (
                        <span className="inline-block px-2 py-0.5 rounded-full text-[10px] font-semibold bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20">
                            {t('build.ssgLatestBadge')}
                        </span>
                    )}
                </div>
            )}
        </>
    );
}
