import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router';
import { useSsgState } from '../api/hooks';
import type { SsgPortInfo, SsgServiceInfo } from '../types/api';

interface SsgPanelProps {
    readonly project: string;
}

interface ServiceRow {
    name: string;
    image: string;
    canonical: number | null;
    dynamic: number | null;
    virtual: number | null;
    status: string;
}

/**
 * Merge `services` (from `Ps`) and `ports` (from `Ports`) into a
 * single per-service row. Both lists key by service `name`. The
 * `services` list is the source-of-truth for what services exist
 * (it's read from the build's manifest); ports may be empty when
 * the SSG hasn't run yet.
 */
function mergeRows(
    services: readonly SsgServiceInfo[],
    ports: readonly SsgPortInfo[],
): ServiceRow[] {
    const portByName = new Map<string, SsgPortInfo>();
    for (const p of ports) {
        portByName.set(p.service, p);
    }
    return services.map((s) => {
        const port = portByName.get(s.name);
        return {
            name: s.name,
            image: s.image,
            canonical: port?.canonical_port ?? s.inner_port ?? null,
            dynamic: port?.dynamic_host_port ?? s.dynamic_host_port ?? null,
            virtual: port?.virtual_port ?? null,
            status: s.status,
        };
    });
}

export default function SsgPanel({ project }: SsgPanelProps) {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const { data: ssgState, isLoading, error } = useSsgState(project);

    const rows: ServiceRow[] = useMemo(
        () =>
            ssgState
                ? mergeRows(ssgState.services, ssgState.ports)
                : [],
        [ssgState],
    );

    if (isLoading) {
        return (
            <section className="mt-1">
                <div className="glass-panel p-6 text-sm text-subtle-ui">Loading…</div>
            </section>
        );
    }

    if (error != null) {
        return (
            <section className="mt-1">
                <div className="glass-panel p-6 text-sm text-rose-600 dark:text-rose-400">
                    {error instanceof Error ? error.message : String(error)}
                </div>
            </section>
        );
    }

    const noBuildYet =
        ssgState != null &&
        ssgState.latest_build_id == null &&
        ssgState.services.length === 0;

    if (noBuildYet) {
        return (
            <section className="mt-1">
                <div className="glass-panel p-6 text-sm text-subtle-ui">
                    {t('ssg.notBuiltYet')}
                </div>
            </section>
        );
    }

    return (
        <section className="mt-1 space-y-4">
            <div className="glass-panel p-5">
                <h3 className="text-sm font-semibold text-main mb-3">
                    {t('ssg.statusHeader')}
                </h3>
                <div className="grid grid-cols-2 gap-y-2 gap-x-6 text-sm">
                    <span className="text-subtle-ui">{t('ssg.containerStatus')}</span>
                    <span className="text-main">
                        <StatusBadge status={ssgState?.status ?? null} t={t} />
                    </span>
                    {ssgState?.latest_build_id != null && (
                        <>
                            <span className="text-subtle-ui">
                                {t('build.ssgLatestBadge')}
                            </span>
                            <button
                                type="button"
                                className="text-left font-mono text-xs break-all text-[var(--primary)] hover:text-[var(--primary-strong)] hover:underline transition-colors cursor-pointer bg-transparent border-0 p-0"
                                onClick={() =>
                                    navigate(
                                        `/project/${project}/ssg-builds/${encodeURIComponent(
                                            ssgState.latest_build_id ?? '',
                                        )}`,
                                    )
                                }
                            >
                                {ssgState.latest_build_id}
                            </button>
                        </>
                    )}
                    {ssgState?.pinned_build_id != null && (
                        <>
                            <span className="text-subtle-ui">
                                {t('build.ssgPinnedBadge')}
                            </span>
                            <button
                                type="button"
                                className="text-left font-mono text-xs break-all text-[var(--primary)] hover:text-[var(--primary-strong)] hover:underline transition-colors cursor-pointer bg-transparent border-0 p-0"
                                onClick={() =>
                                    navigate(
                                        `/project/${project}/ssg-builds/${encodeURIComponent(
                                            ssgState.pinned_build_id ?? '',
                                        )}`,
                                    )
                                }
                            >
                                {ssgState.pinned_build_id}
                            </button>
                        </>
                    )}
                </div>
                {ssgState?.message && (
                    <p className="mt-3 text-xs text-subtle-ui">{ssgState.message}</p>
                )}
            </div>

            {rows.length > 0 && (
                <div className="glass-panel overflow-hidden">
                    <h3 className="text-sm font-semibold text-main px-5 pt-4 pb-2">
                        {t('ssg.servicesHeader')} ({rows.length})
                    </h3>
                    <div className="overflow-x-auto">
                        <table className="w-full text-sm">
                            <thead>
                                <tr className="border-b border-[var(--border)] text-left text-xs text-subtle-ui">
                                    <th className="px-5 py-2 font-medium">
                                        {t('col.name')}
                                    </th>
                                    <th className="px-4 py-2 font-medium">
                                        {t('build.image')}
                                    </th>
                                    <th className="px-4 py-2 font-medium">
                                        {t('ssg.canonical')}
                                    </th>
                                    <th className="px-4 py-2 font-medium">
                                        {t('ssg.dynamic')}
                                    </th>
                                    <th className="px-4 py-2 font-medium">
                                        {t('ssg.virtual')}
                                    </th>
                                    <th className="px-4 py-2 font-medium">
                                        {t('col.status')}
                                    </th>
                                </tr>
                            </thead>
                            <tbody className="divide-y divide-[var(--border)]">
                                {rows.map((r) => (
                                    <tr key={r.name}>
                                        <td className="px-5 py-2.5 font-mono text-xs text-main">
                                            {r.name}
                                        </td>
                                        <td className="px-4 py-2.5 font-mono text-xs">
                                            {r.image}
                                        </td>
                                        <td className="px-4 py-2.5 font-mono text-xs text-subtle-ui">
                                            {r.canonical ?? '—'}
                                        </td>
                                        <td className="px-4 py-2.5 font-mono text-xs text-subtle-ui">
                                            {r.dynamic && r.dynamic > 0 ? r.dynamic : '—'}
                                        </td>
                                        <td className="px-4 py-2.5 font-mono text-xs text-subtle-ui">
                                            {r.virtual ?? '—'}
                                        </td>
                                        <td className="px-4 py-2.5 text-xs">
                                            <ServiceStatusPill status={r.status} />
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    </div>
                </div>
            )}
        </section>
    );
}

function StatusBadge({
    status,
    t,
}: {
    readonly status: string | null;
    readonly t: ReturnType<typeof useTranslation>['t'];
}) {
    if (status == null) {
        return <span className="text-subtle-ui">{t('ssg.statusAbsent')}</span>;
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

function ServiceStatusPill({ status }: { readonly status: string }) {
    const color =
        status === 'running'
            ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
            : status === 'built'
                ? 'bg-amber-500/10 text-amber-600 dark:text-amber-400'
                : 'bg-rose-500/10 text-rose-600 dark:text-rose-400';
    return (
        <span
            className={`px-1.5 py-0.5 rounded font-mono text-[10px] uppercase ${color}`}
        >
            {status}
        </span>
    );
}
