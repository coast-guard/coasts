import { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router';
import { useQueryClient } from '@tanstack/react-query';
import DataTable, { type Column } from './DataTable';
import Toolbar, { type ToolbarAction } from './Toolbar';
import ConfirmModal from './ConfirmModal';
import { api } from '../api/endpoints';
import { useSsgState } from '../api/hooks';

interface SsgListPanelProps {
    readonly project: string;
    readonly navigate: (path: string) => void;
}

interface SsgRow {
    /** Logical instance name. Always `"local"` for v1; the slot is
     *  reserved for future per-host SSG variants. */
    name: string;
    status: string;
    latestBuildId: string | null;
    pinnedBuildId: string | null;
    serviceCount: number;
}

/**
 * Project-detail "SSG" tab content. Mirrors the Coasts tab's
 * Toolbar + DataTable shape but for the project's per-project
 * SSG. Today there's exactly one row (`"local"`) — clicking it
 * navigates to `/project/<p>/ssg/local`.
 *
 * The Toolbar exposes Stop/Start/Remove actions identical in
 * behaviour to the Coasts list. Each action posts to the
 * `/api/v1/ssg/{stop,start,run,rm}` endpoint and refreshes the
 * `ssgState` query on completion. Remove is gated by a
 * {@link ConfirmModal}.
 *
 * Future expansion: when SSGs gain per-host variants (e.g. a dev
 * VM running its own SSG mirror), additional rows can be added
 * here without touching the detail page.
 */
export default function SsgListPanel({ project, navigate }: SsgListPanelProps) {
    const { t } = useTranslation();
    const queryClient = useQueryClient();
    const { data, isLoading, error } = useSsgState(project);

    const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(
        () => new Set<string>(),
    );
    const [confirmRemove, setConfirmRemove] = useState(false);
    const [actionPending, setActionPending] = useState(false);
    const [actionError, setActionError] = useState<string | null>(null);

    const rows: readonly SsgRow[] = useMemo(() => {
        if (data == null) {
            return [];
        }
        // Only render a row when an SSG container actually exists
        // (running OR stopped). The daemon's `absent` sentinel is
        // an internal "runtime cleared but build pointer preserved"
        // marker — surfacing it as a table row is misleading
        // because there's nothing the user can do TO that row
        // (Stop/Start are no-ops on a missing container; the
        // page-level "Run SSG" button is the only relevant action).
        // Map both `absent` and `null` to the empty-state panel
        // below.
        const status = data.status;
        const hasRuntime = status != null && status !== 'absent';
        if (!hasRuntime) {
            return [];
        }
        return [
            {
                name: 'local',
                status,
                latestBuildId: data.latest_build_id,
                pinnedBuildId: data.pinned_build_id,
                serviceCount: data.services.length,
            },
        ];
    }, [data]);

    /**
     * Run the same action across every selected row, swallowing
     * per-row errors into a single banner so partial successes
     * still refresh the list. Mirrors `sectionBatchAction` from
     * `ProjectDetailPage` but scoped to a single section.
     */
    const runForSelected = useCallback(
        async (verb: (project: string, name: string) => Promise<unknown>) => {
            if (selectedIds.size === 0) {
                return;
            }
            setActionPending(true);
            setActionError(null);
            const errors: string[] = [];
            for (const _name of selectedIds) {
                try {
                    // The SSG is project-scoped, not name-scoped — every
                    // selected row maps to the same `project` arg.
                    // Looping is a no-op cost for the single-row v1
                    // case but keeps the code shape ready for the
                    // future per-host expansion called out in the
                    // panel docstring.
                    await verb(project, _name);
                } catch (e) {
                    errors.push(e instanceof Error ? e.message : String(e));
                }
            }
            setActionPending(false);
            setSelectedIds(new Set<string>());
            if (errors.length > 0) {
                setActionError(errors.join('; '));
            }
            await queryClient.invalidateQueries({ queryKey: ['ssgState', project] });
        },
        [project, queryClient, selectedIds],
    );

    const handleStop = useCallback(
        () => void runForSelected((p) => api.ssgStop(p)),
        [runForSelected],
    );
    const handleStart = useCallback(
        // "Start" maps to either `ssg run` (when the container
        // doesn't exist) or `ssg start` (when it's stopped).
        // Pick based on the row's current status. For `absent` we
        // need Run; for `stopped` we need Start. Anything else is
        // already running and the call is a no-op on the daemon.
        () =>
            void runForSelected((p) => {
                const status = data?.status;
                if (status === 'stopped') {
                    return api.ssgStart(p);
                }
                return api.ssgRun(p);
            }),
        [runForSelected, data?.status],
    );
    const handleRemove = useCallback(
        () => void runForSelected((p) => api.ssgRm(p, { force: true })),
        [runForSelected],
    );

    const toolbarActions: readonly ToolbarAction[] = useMemo(
        () => [
            {
                label: t('action.stop'),
                variant: 'outline' as const,
                onClick: handleStop,
            },
            {
                label: t('action.start'),
                variant: 'outline' as const,
                onClick: handleStart,
            },
            {
                label: t('action.remove'),
                variant: 'danger' as const,
                onClick: () => setConfirmRemove(true),
            },
        ],
        [t, handleStop, handleStart],
    );

    const columns: readonly Column<SsgRow>[] = useMemo(
        () => [
            {
                key: 'name',
                header: t('col.name'),
                className: 'w-32',
                headerClassName: 'w-32',
                render: (r) => (
                    <Link
                        to={`/project/${project}/ssg/${encodeURIComponent(r.name)}`}
                        className="font-medium text-[var(--primary)] hover:text-[var(--primary-strong)] hover:underline"
                        onClick={(e) => e.stopPropagation()}
                    >
                        {r.name}
                    </Link>
                ),
            },
            {
                key: 'status',
                header: t('col.status'),
                className: 'w-32',
                headerClassName: 'w-32',
                render: (r) => <SsgStatusPill status={r.status} />,
            },
            {
                key: 'services',
                header: t('ssg.col.serviceCount'),
                className: 'w-28',
                headerClassName: 'w-28',
                render: (r) => <span className="text-subtle-ui">{r.serviceCount}</span>,
            },
            {
                key: 'build',
                header: t('col.build'),
                className: 'w-auto',
                headerClassName: 'w-auto',
                render: (r) =>
                    r.latestBuildId != null ? (
                        <Link
                            to={`/project/${project}/ssg-builds/${encodeURIComponent(r.latestBuildId)}`}
                            className="font-mono text-xs text-[var(--primary)] hover:text-[var(--primary-strong)] hover:underline"
                            onClick={(e) => e.stopPropagation()}
                        >
                            {r.latestBuildId}
                        </Link>
                    ) : (
                        <span className="text-subtle-ui text-xs">—</span>
                    ),
            },
            {
                key: 'pinned',
                header: t('build.ssgPinnedBadge'),
                className: 'w-auto',
                headerClassName: 'w-auto',
                render: (r) =>
                    r.pinnedBuildId != null ? (
                        <Link
                            to={`/project/${project}/ssg-builds/${encodeURIComponent(r.pinnedBuildId)}`}
                            className="font-mono text-xs text-amber-600 dark:text-amber-400 hover:underline"
                            onClick={(e) => e.stopPropagation()}
                        >
                            {r.pinnedBuildId}
                        </Link>
                    ) : (
                        <span className="text-subtle-ui text-xs">—</span>
                    ),
            },
        ],
        [project, t],
    );

    if (isLoading) {
        return (
            <section className="mt-1 glass-panel p-6 text-sm text-subtle-ui">
                Loading…
            </section>
        );
    }
    if (error != null) {
        return (
            <section className="mt-1 glass-panel p-6 text-sm text-rose-600 dark:text-rose-400">
                {error instanceof Error ? error.message : String(error)}
            </section>
        );
    }
    if (rows.length === 0) {
        // Differentiate "never built" from "built but not running"
        // — the latter is reachable via the page-level "Run SSG"
        // button (no Builds-tab detour needed).
        const hasBuild = data?.latest_build_id != null;
        return (
            <section className="mt-1 glass-panel p-6 text-sm text-subtle-ui">
                {hasBuild ? t('ssg.notRunning') : t('ssg.notBuiltYet')}
            </section>
        );
    }

    const selectedCount = selectedIds.size;

    return (
        <section className="mt-1">
            {actionError != null && (
                <div className="mb-3 rounded-md border border-rose-300 bg-rose-50 px-4 py-2 text-sm text-rose-700 dark:border-rose-700 dark:bg-rose-950/40 dark:text-rose-300">
                    {actionError}
                </div>
            )}
            <div className="glass-panel overflow-hidden">
                <Toolbar
                    actions={toolbarActions}
                    selectedCount={selectedCount}
                />
                <DataTable
                    columns={columns}
                    data={rows}
                    getRowId={(r) => r.name}
                    selectable
                    selectedIds={selectedIds}
                    onSelectionChange={setSelectedIds}
                    onRowClick={(r) =>
                        navigate(`/project/${project}/ssg/${encodeURIComponent(r.name)}`)
                    }
                    emptyMessage={t('ssg.notBuiltYet')}
                />
            </div>

            <ConfirmModal
                open={confirmRemove}
                title={t('ssg.removeTitle')}
                body={t('ssg.removeBody', { count: selectedCount })}
                onConfirm={() => {
                    setConfirmRemove(false);
                    handleRemove();
                }}
                onCancel={() => setConfirmRemove(false)}
                confirmLabel={t('action.remove')}
                danger
            />

            {actionPending && (
                <div className="mt-2 text-xs text-subtle-ui">
                    {t('ssg.actionPending')}
                </div>
            )}
        </section>
    );
}

function SsgStatusPill({ status }: { readonly status: string }) {
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
