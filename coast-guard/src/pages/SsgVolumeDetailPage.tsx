import { useMemo } from 'react';
import { useParams, Link } from 'react-router';
import { useTranslation } from 'react-i18next';
import type { CoastfileVolumeConfig } from '../types/api';
import { useSsgVolumeInspect } from '../api/hooks';
import Breadcrumb from '../components/Breadcrumb';
import Section from '../components/Section';
import KeyValue from '../components/KeyValue';
import StrategyBadge from '../components/StrategyBadge';

/**
 * SSG flavor of {@link VolumeDetailPage}: same UI shell
 * (Configuration / Overview / Labels / Options / Used by
 * Services) reading from `GET /api/v1/ssg/volumes/inspect`. The
 * Coastfile-config block is sourced from the project's active SSG
 * `Coastfile.shared_service_groups` rather than the regular
 * Coastfile.
 */

function safeGet(obj: unknown, ...keys: string[]): unknown {
    let current: unknown = obj;
    for (const key of keys) {
        if (current == null || typeof current !== 'object') return undefined;
        current = (current as Record<string, unknown>)[key];
    }
    return current;
}

function asString(val: unknown): string {
    if (val == null) return '';
    if (typeof val === 'string') return val;
    return JSON.stringify(val);
}

function asRecord(val: unknown): Record<string, string> {
    if (val == null || typeof val !== 'object' || Array.isArray(val)) return {};
    const result: Record<string, string> = {};
    for (const [k, v] of Object.entries(val as Record<string, unknown>)) {
        result[k] = typeof v === 'string' ? v : JSON.stringify(v);
    }
    return result;
}

function extractServiceName(containerName: string, project: string): string {
    const cleaned = containerName.replace(/^\//, '');
    const prefix = `${project}-ssg-`;
    const stripped = cleaned.startsWith(prefix) ? cleaned.slice(prefix.length) : cleaned;
    const match = stripped.match(/^(.+)-(\d+)$/);
    if (match != null) return match[1]!;
    return stripped;
}

export default function SsgVolumeDetailPage() {
    const { t } = useTranslation();
    const params = useParams<{
        project: string;
        volumeName: string;
    }>();
    const project = params.project ?? '';
    const volumeName = decodeURIComponent(params.volumeName ?? '');

    const { data, isLoading, error } = useSsgVolumeInspect(project, volumeName);

    const inspectArr = data?.inspect;
    const inspect =
        Array.isArray(inspectArr) && inspectArr.length > 0
            ? (inspectArr[0] as Record<string, unknown>)
            : null;

    const containers = useMemo(() => {
        if (data?.containers == null) return [];
        return data.containers as Record<string, unknown>[];
    }, [data]);

    const coastfile = (data?.coastfile ?? null) as CoastfileVolumeConfig | null;

    const labels = inspect != null ? asRecord(safeGet(inspect, 'Labels')) : {};
    const options = inspect != null ? asRecord(safeGet(inspect, 'Options')) : {};

    return (
        <div className="page-shell">
            <Breadcrumb
                items={[
                    { label: t('nav.projects'), to: '/' },
                    { label: project, to: `/project/${project}` },
                    { label: t('ssg.breadcrumb.local'), to: `/project/${project}/ssg/local` },
                    { label: t('tab.volumes'), to: `/project/${project}/ssg/local/volumes` },
                    { label: volumeName },
                ]}
            />

            {isLoading && <p className="text-sm text-subtle-ui py-8">{t('volumes.loading')}</p>}

            {error != null && (
                <p className="text-sm text-rose-500 py-8">
                    {t('volumes.loadError', { error: String(error) })}
                </p>
            )}

            {inspect != null && (
                <>
                    <h1 className="text-2xl font-bold text-main mb-1 font-mono break-all">
                        {volumeName}
                    </h1>
                    <p className="text-xs text-subtle-ui mb-6">
                        {asString(safeGet(inspect, 'Driver'))} /{' '}
                        {asString(safeGet(inspect, 'Scope'))}
                    </p>

                    {coastfile != null ? (
                        <Section title={t('volumes.configuration')}>
                            <div className="flex gap-3 py-1.5 border-b border-[var(--border)]">
                                <span className="text-xs text-subtle-ui w-36 shrink-0 font-medium">
                                    {t('volumes.strategy')}
                                </span>
                                <StrategyBadge strategy={coastfile.strategy} />
                            </div>
                            <div className="flex gap-3 py-1.5 border-b border-[var(--border)]">
                                <span className="text-xs text-subtle-ui w-36 shrink-0 font-medium">
                                    {t('volumes.service')}
                                </span>
                                <Link
                                    to={`/project/${project}/ssg/local/services`}
                                    className="text-xs font-mono text-[var(--primary)] hover:underline"
                                >
                                    {coastfile.service}
                                </Link>
                            </div>
                            <KeyValue label={t('volumes.mount')} value={coastfile.mount} />
                            {coastfile.snapshot_source != null && (
                                <div className="flex gap-3 py-1.5 border-b border-[var(--border)] last:border-0">
                                    <span className="text-xs text-subtle-ui w-36 shrink-0 font-medium">
                                        {t('volumes.snapshotSource')}
                                    </span>
                                    <Link
                                        to={`/project/${project}/ssg/local/volumes/${encodeURIComponent(coastfile.snapshot_source)}`}
                                        className="text-xs font-mono text-[var(--primary)] hover:underline"
                                    >
                                        {coastfile.snapshot_source}
                                    </Link>
                                </div>
                            )}
                        </Section>
                    ) : (
                        <Section title={t('volumes.configuration')}>
                            <p className="text-xs text-subtle-ui">{t('volumes.notConfigured')}</p>
                        </Section>
                    )}

                    <Section title={t('volumes.overview')}>
                        <KeyValue
                            label={t('volumes.name')}
                            value={asString(safeGet(inspect, 'Name'))}
                        />
                        <KeyValue
                            label={t('volumes.driver')}
                            value={asString(safeGet(inspect, 'Driver'))}
                        />
                        <KeyValue
                            label={t('volumes.scope')}
                            value={asString(safeGet(inspect, 'Scope'))}
                        />
                        <KeyValue
                            label={t('volumes.mountpoint')}
                            value={asString(safeGet(inspect, 'Mountpoint'))}
                        />
                        <KeyValue
                            label={t('volumes.createdAt')}
                            value={asString(safeGet(inspect, 'CreatedAt'))}
                        />
                    </Section>

                    {Object.keys(labels).length > 0 && (
                        <Section title={t('volumes.labels')}>
                            <div className="max-h-60 overflow-auto">
                                <table className="w-full text-xs">
                                    <thead>
                                        <tr className="border-b border-[var(--border)]">
                                            <th className="text-left py-1.5 pr-4 text-subtle-ui font-semibold uppercase tracking-wider">
                                                Key
                                            </th>
                                            <th className="text-left py-1.5 text-subtle-ui font-semibold uppercase tracking-wider">
                                                Value
                                            </th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {Object.entries(labels).map(([key, val]) => (
                                            <tr
                                                key={key}
                                                className="border-b border-[var(--border)] last:border-0"
                                            >
                                                <td className="py-1.5 pr-4 font-mono font-medium text-main">
                                                    {key}
                                                </td>
                                                <td className="py-1.5 font-mono text-subtle-ui break-all">
                                                    {val}
                                                </td>
                                            </tr>
                                        ))}
                                    </tbody>
                                </table>
                            </div>
                        </Section>
                    )}

                    {Object.keys(options).length > 0 && (
                        <Section title={t('volumes.options')}>
                            <div className="max-h-60 overflow-auto">
                                <table className="w-full text-xs">
                                    <thead>
                                        <tr className="border-b border-[var(--border)]">
                                            <th className="text-left py-1.5 pr-4 text-subtle-ui font-semibold uppercase tracking-wider">
                                                Key
                                            </th>
                                            <th className="text-left py-1.5 text-subtle-ui font-semibold uppercase tracking-wider">
                                                Value
                                            </th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {Object.entries(options).map(([key, val]) => (
                                            <tr
                                                key={key}
                                                className="border-b border-[var(--border)] last:border-0"
                                            >
                                                <td className="py-1.5 pr-4 font-mono font-medium text-main">
                                                    {key}
                                                </td>
                                                <td className="py-1.5 font-mono text-subtle-ui break-all">
                                                    {val}
                                                </td>
                                            </tr>
                                        ))}
                                    </tbody>
                                </table>
                            </div>
                        </Section>
                    )}

                    <Section title={t('volumes.usedBy')}>
                        {containers.length === 0 ? (
                            <p className="text-xs text-subtle-ui">{t('volumes.noContainers')}</p>
                        ) : (
                            <div className="space-y-2">
                                {containers.map((c, i) => {
                                    const cName = asString(safeGet(c, 'Names')).replace(/^\//, '');
                                    const serviceName = extractServiceName(cName, project);
                                    const state = asString(safeGet(c, 'State'));
                                    const isRunning = state === 'running';
                                    return (
                                        <div
                                            key={i}
                                            className="flex items-center gap-3 py-2 border-b border-[var(--border)] last:border-0"
                                        >
                                            <span
                                                className={`h-1.5 w-1.5 rounded-full shrink-0 ${
                                                    isRunning ? 'bg-emerald-500' : 'bg-slate-400'
                                                }`}
                                            />
                                            <Link
                                                to={`/project/${project}/ssg/local/services`}
                                                className="font-mono text-xs text-[var(--primary)] hover:underline"
                                            >
                                                {serviceName}
                                            </Link>
                                            <span className="font-mono text-[10px] text-subtle-ui">
                                                ({cName})
                                            </span>
                                            <span
                                                className={`text-[10px] ${
                                                    isRunning
                                                        ? 'text-emerald-600 dark:text-emerald-400'
                                                        : 'text-subtle-ui'
                                                }`}
                                            >
                                                {state}
                                            </span>
                                        </div>
                                    );
                                })}
                            </div>
                        )}
                    </Section>
                </>
            )}
        </div>
    );
}
