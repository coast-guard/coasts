import { useMemo } from 'react';
import { useParams, Link } from 'react-router';
import { useTranslation } from 'react-i18next';
import { projectName, instanceName } from '../types/branded';
import type { CoastfileVolumeConfig } from '../types/api';
import { useVolumeInspect } from '../api/hooks';
import Breadcrumb from '../components/Breadcrumb';
import Section from '../components/Section';
import KeyValue from '../components/KeyValue';
import StrategyBadge from '../components/StrategyBadge';

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

function extractServiceName(containerName: string): string {
  const cleaned = containerName.replace(/^\//, '');
  const match = cleaned.match(/^(.+)-(\d+)$/);
  if (match != null) {
    const prefix = match[1]!;
    const lastDash = prefix.lastIndexOf('-');
    if (lastDash > 0) return prefix.slice(lastDash + 1);
    return prefix;
  }
  return cleaned;
}

export default function VolumeDetailPage() {
  const { t } = useTranslation();
  const params = useParams<{
    project: string;
    name: string;
    volumeName: string;
  }>();
  const project = projectName(params.project ?? '');
  const name = instanceName(params.name ?? '');
  const volumeName = decodeURIComponent(params.volumeName ?? '');

  const { data, isLoading, error } = useVolumeInspect(project, name, volumeName);

  const inspectArr = data?.inspect;
  const inspect = Array.isArray(inspectArr) && inspectArr.length > 0
    ? inspectArr[0] as Record<string, unknown>
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
          { label: name, to: `/instance/${project}/${name}` },
          { label: t('tab.volumes'), to: `/instance/${project}/${name}/volumes` },
          { label: volumeName },
        ]}
      />

      {isLoading && (
        <p className="text-sm text-subtle-ui py-8">{t('volumes.loading')}</p>
      )}

      {error != null && (
        <p className="text-sm text-rose-500 py-8">{t('volumes.loadError', { error: String(error) })}</p>
      )}

      {inspect != null && (
        <>
          <h1 className="text-2xl font-bold text-main mb-1 font-mono break-all">{volumeName}</h1>
          <p className="text-xs text-subtle-ui mb-6">
            {asString(safeGet(inspect, 'Driver'))} / {asString(safeGet(inspect, 'Scope'))}
          </p>

          {coastfile != null ? (
            <Section title={t('volumes.configuration')}>
              <div className="flex gap-3 py-1.5 border-b border-[var(--border)]">
                <span className="text-xs text-subtle-ui w-36 shrink-0 font-medium">{t('volumes.strategy')}</span>
                <StrategyBadge strategy={coastfile.strategy} />
              </div>
              <div className="flex gap-3 py-1.5 border-b border-[var(--border)]">
                <span className="text-xs text-subtle-ui w-36 shrink-0 font-medium">{t('volumes.service')}</span>
                <Link
                  to={`/instance/${project}/${name}/services/${encodeURIComponent(coastfile.service)}`}
                  className="text-xs font-mono text-[var(--primary)] hover:underline"
                >
                  {coastfile.service}
                </Link>
              </div>
              <KeyValue label={t('volumes.mount')} value={coastfile.mount} />
              {coastfile.snapshot_source != null && (
                <div className="flex gap-3 py-1.5 border-b border-[var(--border)] last:border-0">
                  <span className="text-xs text-subtle-ui w-36 shrink-0 font-medium">{t('volumes.snapshotSource')}</span>
                  <Link
                    to={`/instance/${project}/${name}/volumes/${encodeURIComponent(coastfile.snapshot_source)}`}
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
            <KeyValue label={t('volumes.name')} value={asString(safeGet(inspect, 'Name'))} />
            <KeyValue label={t('volumes.driver')} value={asString(safeGet(inspect, 'Driver'))} />
            <KeyValue label={t('volumes.scope')} value={asString(safeGet(inspect, 'Scope'))} />
            <KeyValue label={t('volumes.mountpoint')} value={asString(safeGet(inspect, 'Mountpoint'))} />
            <KeyValue label={t('volumes.createdAt')} value={asString(safeGet(inspect, 'CreatedAt'))} />
          </Section>

          {Object.keys(labels).length > 0 && (
            <Section title={t('volumes.labels')}>
              <div className="max-h-60 overflow-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-[var(--border)]">
                      <th className="text-left py-1.5 pr-4 text-subtle-ui font-semibold uppercase tracking-wider">Key</th>
                      <th className="text-left py-1.5 text-subtle-ui font-semibold uppercase tracking-wider">Value</th>
                    </tr>
                  </thead>
                  <tbody>
                    {Object.entries(labels).map(([key, val]) => (
                      <tr key={key} className="border-b border-[var(--border)] last:border-0">
                        <td className="py-1.5 pr-4 font-mono font-medium text-main">{key}</td>
                        <td className="py-1.5 font-mono text-subtle-ui break-all">{val}</td>
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
                      <th className="text-left py-1.5 pr-4 text-subtle-ui font-semibold uppercase tracking-wider">Key</th>
                      <th className="text-left py-1.5 text-subtle-ui font-semibold uppercase tracking-wider">Value</th>
                    </tr>
                  </thead>
                  <tbody>
                    {Object.entries(options).map(([key, val]) => (
                      <tr key={key} className="border-b border-[var(--border)] last:border-0">
                        <td className="py-1.5 pr-4 font-mono font-medium text-main">{key}</td>
                        <td className="py-1.5 font-mono text-subtle-ui break-all">{val}</td>
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
                  const serviceName = extractServiceName(cName);
                  const state = asString(safeGet(c, 'State'));
                  const isRunning = state === 'running';
                  return (
                    <div key={i} className="flex items-center gap-3 py-2 border-b border-[var(--border)] last:border-0">
                      <span className={`h-1.5 w-1.5 rounded-full shrink-0 ${isRunning ? 'bg-emerald-500' : 'bg-slate-400'}`} />
                      <Link
                        to={`/instance/${project}/${name}/services/${encodeURIComponent(serviceName)}`}
                        className="font-mono text-xs text-[var(--primary)] hover:underline"
                      >
                        {serviceName}
                      </Link>
                      <span className="font-mono text-[10px] text-subtle-ui">({cName})</span>
                      <span className={`text-[10px] ${isRunning ? 'text-emerald-600 dark:text-emerald-400' : 'text-subtle-ui'}`}>
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
