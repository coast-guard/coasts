import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { SsgPortInfo } from '../../types/api';
import { useSsgState } from '../../api/hooks';
import DataTable, { type Column } from '../DataTable';
import HealthDot from '../HealthDot';

interface SsgPortsTabProps {
    readonly project: string;
}

type SsgPortRow = SsgPortInfo;

/**
 * SSG → Ports tab. Visual parity with `InstancePortsTab` minus
 * a few affordances that don't apply to SSGs:
 *
 * - **No primary-port star.** SSGs are shared services consumed
 *   by multiple coasts; "primary port" is a per-instance concept.
 * - **No URL template editor.** SSGs ship a fixed service set
 *   from the Coastfile.shared_service_groups; rewriting URLs
 *   per-service has no analogue at the SSG layer.
 * - **No subdomain-routing banner.** Cookie collisions are an
 *   instance-side problem (the SPA itself runs on `localhost`);
 *   SSG dynamic ports are typically hit from inner consumer
 *   containers via the virtual-port hop, not from the host
 *   browser, so the toggle would mislead users.
 * - **No VIRTUAL column.** Internal routing detail.
 *
 * Service-level health is shown as a green/red dot in the SERVICE
 * column, derived from the per-service `status` field on
 * `/api/v1/ssg/state` — `running` → green, anything else → red.
 *
 * Data comes from the existing `/api/v1/ssg/state` endpoint —
 * no separate fetch needed.
 */
export default function SsgPortsTab({ project }: SsgPortsTabProps) {
    const { t } = useTranslation();
    const { data, isLoading, error } = useSsgState(project);

    const ports: readonly SsgPortRow[] = useMemo(
        () => data?.ports ?? [],
        [data?.ports],
    );

    // Map service name → runtime status so the SERVICE column can
    // render a HealthDot without an extra lookup per render.
    const serviceStatusByName = useMemo(() => {
        const m = new Map<string, string>();
        for (const svc of data?.services ?? []) {
            m.set(svc.name, svc.status);
        }
        return m;
    }, [data?.services]);

    const columns: readonly Column<SsgPortRow>[] = useMemo(
        () => [
            {
                key: 'service',
                header: t('col.service'),
                render: (r) => {
                    const status = serviceStatusByName.get(r.service);
                    // The HealthDot expects `boolean | undefined`.
                    // Map known statuses; leave `undefined` (the
                    // grey "checking" pill) only when the SSG row
                    // exists but services haven't been published
                    // yet — practically a transient state during
                    // run/stop transitions.
                    const healthy =
                        status == null
                            ? undefined
                            : status === 'running';
                    return (
                        <div className="flex items-center gap-2">
                            <HealthDot healthy={healthy} />
                            <span className="font-medium">{r.service}</span>
                        </div>
                    );
                },
            },
            {
                key: 'canonical',
                header: t('col.canonical'),
                render: (r) => {
                    const url = `http://localhost:${r.canonical_port}`;
                    if (!r.checked_out) {
                        // Canonical port is not bound on the host
                        // until `coast ssg checkout <service>`; show
                        // it as muted text so users see where it
                        // *will* be reachable without misleading
                        // them into clicking a dead link.
                        return (
                            <span className="font-mono text-xs text-subtle-ui">
                                {url}
                            </span>
                        );
                    }
                    return (
                        <a
                            href={url}
                            target="_blank"
                            rel="noopener noreferrer"
                            onClick={(e) => e.stopPropagation()}
                            className="font-mono text-xs text-[var(--primary)] hover:underline"
                        >
                            {url}
                        </a>
                    );
                },
            },
            {
                key: 'dynamic',
                header: t('col.dynamic'),
                render: (r) => {
                    if (r.dynamic_host_port <= 0) {
                        return (
                            <span className="font-mono text-xs text-subtle-ui">—</span>
                        );
                    }
                    const url = `http://localhost:${r.dynamic_host_port}`;
                    return (
                        <a
                            href={url}
                            target="_blank"
                            rel="noopener noreferrer"
                            onClick={(e) => e.stopPropagation()}
                            className="font-mono text-xs text-[var(--primary)] hover:underline"
                        >
                            {url}
                        </a>
                    );
                },
            },
        ],
        [t, serviceStatusByName],
    );

    if (isLoading) {
        return (
            <p className="text-sm text-subtle-ui py-4">{t('ports.loading')}</p>
        );
    }
    if (error != null) {
        return (
            <p className="text-sm text-rose-500 py-4">
                {t('ports.loadError', { error: String(error) })}
            </p>
        );
    }

    return (
        <div className="glass-panel overflow-hidden">
            <DataTable
                columns={columns}
                data={ports}
                getRowId={(r) => `${r.service}:${r.canonical_port}`}
                emptyMessage={t('ssg.noPorts')}
            />
        </div>
    );
}
