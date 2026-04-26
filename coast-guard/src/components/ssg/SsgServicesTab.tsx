import { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Warning } from '@phosphor-icons/react';
import type { SsgServiceInfo, SsgPortInfo } from '../../types/api';
import {
    useSsgState,
    useSsgServiceStopMutation,
    useSsgServiceStartMutation,
    useSsgServiceRestartMutation,
    useSsgServiceRmMutation,
} from '../../api/hooks';
import { ApiError } from '../../api/client';
import DataTable, { type Column } from '../DataTable';
import Toolbar, { type ToolbarAction } from '../Toolbar';
import Modal from '../Modal';

interface SsgServicesTabProps {
    readonly project: string;
}

/**
 * SSG → Services tab. Visual parity with `InstanceServicesTab`:
 * same Toolbar (Stop / Start / Restart / Remove), same DataTable
 * shape (SERVICE / STATUS / IMAGE / PORTS), same selection
 * semantics. Per-service control flows through the
 * `/api/v1/ssg/services/{stop,start,restart,rm}` endpoints which
 * `docker exec` into the outer DinD container and run the matching
 * `docker compose <verb> <service>` against the inner compose
 * stack.
 *
 * Service rows are NOT clickable — drilling into a per-service
 * detail page is intentionally deferred (the user flagged it as a
 * separate phase). The SERVICE column shows the name without a
 * link so the row checkbox is the only interactive surface.
 *
 * Data source: the existing `/api/v1/ssg/state` payload bundles
 * `services[]` (image, status, port mappings) and `ports[]` (host
 * port info) — no separate fetch.
 */
export default function SsgServicesTab({ project }: SsgServicesTabProps) {
    const { t, i18n } = useTranslation();
    const { data, isLoading, error } = useSsgState(project);

    const stopMut = useSsgServiceStopMutation();
    const startMut = useSsgServiceStartMutation();
    const restartMut = useSsgServiceRestartMutation();
    const rmMut = useSsgServiceRmMutation();

    const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(
        () => new Set<string>(),
    );
    const [errorMsg, setErrorMsg] = useState<string | null>(null);

    const services: readonly SsgServiceInfo[] = useMemo(
        () => data?.services ?? [],
        [data?.services],
    );

    // Map service name → port info so the PORTS column can render
    // canonical/dynamic without re-walking `data.ports` per row.
    const portByService = useMemo(() => {
        const m = new Map<string, SsgPortInfo>();
        for (const p of data?.ports ?? []) {
            m.set(p.service, p);
        }
        return m;
    }, [data?.ports]);

    const selectedNames = useMemo(
        () => services.filter((s) => selectedIds.has(s.name)).map((s) => s.name),
        [services, selectedIds],
    );

    const batchAction = useCallback(
        async (
            action: (vars: {
                project: string;
                service: string;
            }) => Promise<unknown>,
        ) => {
            const errors: string[] = [];
            for (const svc of selectedNames) {
                try {
                    await action({ project, service: svc });
                } catch (e) {
                    errors.push(
                        `${svc}: ${e instanceof ApiError ? e.body.error : String(e)}`,
                    );
                }
            }
            setSelectedIds(new Set());
            if (errors.length > 0) {
                setErrorMsg(errors.join('\n'));
            }
        },
        [selectedNames, project],
    );

    const toolbarActions: readonly ToolbarAction[] = useMemo(
        () => [
            {
                label: t('action.stop'),
                variant: 'outline' as const,
                onClick: () =>
                    void batchAction((v) => stopMut.mutateAsync(v)),
            },
            {
                label: t('action.start'),
                variant: 'outline' as const,
                onClick: () =>
                    void batchAction((v) => startMut.mutateAsync(v)),
            },
            {
                label: t('service.restart'),
                variant: 'outline' as const,
                onClick: () =>
                    void batchAction((v) => restartMut.mutateAsync(v)),
            },
            {
                label: t('action.remove'),
                variant: 'danger' as const,
                onClick: () => void batchAction((v) => rmMut.mutateAsync(v)),
            },
        ],
        [batchAction, stopMut, startMut, restartMut, rmMut, t, i18n.language],
    );

    const columns: readonly Column<SsgServiceInfo>[] = useMemo(
        () => [
            {
                key: 'name',
                header: t('col.service'),
                headerClassName: 'w-[22%]',
                className: 'w-[22%]',
                render: (r) => {
                    const isDown = r.status !== 'running';
                    return (
                        <span className="inline-flex items-center gap-2">
                            <span className="font-medium">{r.name}</span>
                            {isDown && (
                                <Warning
                                    size={14}
                                    weight="fill"
                                    className="text-amber-500 shrink-0"
                                />
                            )}
                        </span>
                    );
                },
            },
            {
                key: 'status',
                header: t('col.status'),
                headerClassName: 'w-[12%]',
                className: 'w-[12%]',
                render: (r) => {
                    const isRunning = r.status === 'running';
                    return (
                        <span
                            className={`inline-flex items-center gap-1.5 text-xs ${isRunning
                                ? 'text-emerald-600 dark:text-emerald-400'
                                : 'text-subtle-ui'
                                }`}
                        >
                            <span
                                className={`h-1.5 w-1.5 rounded-full ${isRunning ? 'bg-emerald-500' : 'bg-slate-400'
                                    }`}
                            />
                            {r.status}
                        </span>
                    );
                },
            },
            {
                key: 'image',
                header: t('col.image'),
                headerClassName: 'w-[34%]',
                className: 'w-[34%]',
                render: (r) => (
                    <span
                        className="font-mono text-xs text-subtle-ui truncate max-w-[200px] inline-block"
                        title={r.image}
                    >
                        {r.image}
                    </span>
                ),
            },
            {
                key: 'ports',
                header: t('col.ports'),
                render: (r) => {
                    const port = portByService.get(r.name);
                    if (port == null) {
                        return <span className="text-subtle-ui text-xs">—</span>;
                    }
                    const dynUrl = `http://localhost:${port.dynamic_host_port}`;
                    const canonicalDisabled = !port.checked_out;
                    return (
                        <div className="text-xs font-mono leading-5 flex items-center gap-2">
                            {canonicalDisabled ? (
                                <span className="text-subtle-ui">
                                    :{port.canonical_port}
                                </span>
                            ) : (
                                <a
                                    href={`http://localhost:${port.canonical_port}`}
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    className="text-[var(--primary)] hover:underline"
                                    onClick={(e) => e.stopPropagation()}
                                >
                                    :{port.canonical_port}
                                </a>
                            )}
                            <span className="text-subtle-ui">/</span>
                            {port.dynamic_host_port > 0 ? (
                                <a
                                    href={dynUrl}
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    className="text-[var(--primary)] hover:underline"
                                    onClick={(e) => e.stopPropagation()}
                                >
                                    :{port.dynamic_host_port}
                                </a>
                            ) : (
                                <span className="text-subtle-ui">—</span>
                            )}
                        </div>
                    );
                },
            },
        ],
        [t, portByService],
    );

    const downSvcs = useMemo(
        () => services.filter((s) => s.status !== 'running'),
        [services],
    );

    if (isLoading) {
        return (
            <p className="text-sm text-subtle-ui py-4">
                {t('services.loading')}
            </p>
        );
    }
    if (error != null) {
        return (
            <p className="text-sm text-rose-500 py-4">
                {t('services.loadError', { error: String(error) })}
            </p>
        );
    }

    return (
        <>
            {downSvcs.length > 0 && (
                <div className="flex items-start gap-2 px-3 py-2.5 mb-3 rounded-lg bg-amber-500/10 border border-amber-500/30 text-amber-700 dark:text-amber-300 text-xs">
                    <Warning size={14} weight="fill" className="shrink-0 mt-0.5" />
                    <span>
                        {downSvcs.length} service{downSvcs.length !== 1 ? 's' : ''} not
                        running:{' '}
                        {downSvcs.map((s) => `${s.name} (${s.status})`).join(', ')}
                    </span>
                </div>
            )}
            <div className="glass-panel overflow-hidden">
                <Toolbar
                    actions={toolbarActions}
                    selectedCount={selectedNames.length}
                />
                <DataTable
                    columns={columns}
                    data={services}
                    getRowId={(r) => r.name}
                    selectable
                    selectedIds={selectedIds}
                    onSelectionChange={setSelectedIds}
                    emptyMessage={t('ssg.noServices')}
                />
            </div>

            <Modal
                open={errorMsg != null}
                title={t('error.title')}
                onClose={() => setErrorMsg(null)}
            >
                <p className="text-rose-600 dark:text-rose-400 whitespace-pre-wrap">
                    {errorMsg}
                </p>
            </Modal>
        </>
    );
}
