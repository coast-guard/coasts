import { useMemo } from 'react';
import { useParams, Link } from 'react-router';
import { useTranslation } from 'react-i18next';
import { projectName, instanceName } from '../types/branded';
import { useImageInspect } from '../api/hooks';
import Breadcrumb from '../components/Breadcrumb';
import Section from '../components/Section';
import KeyValue from '../components/KeyValue';

function truncateId(id: string): string {
  const sha = id.startsWith('sha256:') ? id.slice(7) : id;
  return sha.slice(0, 12);
}

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

function asStringArray(val: unknown): readonly string[] {
  if (!Array.isArray(val)) return [];
  return val.map((v) => (typeof v === 'string' ? v : JSON.stringify(v)));
}

function asRecord(val: unknown): Record<string, string> {
  if (val == null || typeof val !== 'object' || Array.isArray(val)) return {};
  const result: Record<string, string> = {};
  for (const [k, v] of Object.entries(val as Record<string, unknown>)) {
    result[k] = typeof v === 'string' ? v : JSON.stringify(v);
  }
  return result;
}

interface HistoryEntry {
  readonly created: string;
  readonly created_by: string;
  readonly size: number;
  readonly empty_layer?: boolean;
}

function asHistory(val: unknown): readonly HistoryEntry[] {
  if (!Array.isArray(val)) return [];
  return val.map((v) => ({
    created: asString(safeGet(v, 'created')),
    created_by: asString(safeGet(v, 'created_by')),
    size: typeof (v as Record<string, unknown>)?.['size'] === 'number'
      ? (v as Record<string, unknown>)['size'] as number
      : 0,
    empty_layer: (v as Record<string, unknown>)?.['empty_layer'] === true,
  }));
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function truncateCommand(cmd: string, maxLen: number = 120): string {
  const cleaned = cmd.replace(/^\/bin\/sh -c /, '').replace(/#\(nop\)\s+/g, '');
  return cleaned.length > maxLen ? cleaned.slice(0, maxLen) + '...' : cleaned;
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

export default function ImageDetailPage() {
  const { t } = useTranslation();
  const params = useParams<{
    project: string;
    name: string;
    imageId: string;
  }>();
  const project = projectName(params.project ?? '');
  const name = instanceName(params.name ?? '');
  const imageId = params.imageId ?? '';

  const { data, isLoading, error } = useImageInspect(project, name, imageId);

  const inspectArr = (data as { inspect?: unknown; containers?: unknown[] } | undefined)?.inspect;
  const inspect = Array.isArray(inspectArr) && inspectArr.length > 0 ? inspectArr[0] as Record<string, unknown> : null;

  const containers = useMemo(() => {
    const raw = (data as { containers?: unknown[] } | undefined)?.containers;
    if (!Array.isArray(raw)) return [];
    return raw as Record<string, unknown>[];
  }, [data]);

  const repoTags = inspect != null ? asStringArray(safeGet(inspect, 'RepoTags')) : [];
  const displayName = repoTags.length > 0 ? repoTags[0]! : truncateId(imageId);
  const fullId = asString(safeGet(inspect, 'Id'));

  const config = inspect != null ? (safeGet(inspect, 'Config') as Record<string, unknown> | undefined) : undefined;
  const history = inspect != null ? asHistory(safeGet(inspect, 'History')) : [];

  return (
    <div className="page-shell">
      <Breadcrumb
        items={[
          { label: t('nav.projects'), to: '/' },
          { label: project, to: `/project/${project}` },
          { label: name, to: `/instance/${project}/${name}` },
          { label: t('tab.images'), to: `/instance/${project}/${name}/images` },
          { label: truncateId(imageId) },
        ]}
      />

      {isLoading && (
        <p className="text-sm text-subtle-ui py-8">{t('images.loading')}</p>
      )}

      {error != null && (
        <p className="text-sm text-rose-500 py-8">{t('images.loadError', { error: String(error) })}</p>
      )}

      {inspect != null && (
        <>
          <h1 className="text-2xl font-bold text-main mb-1">{displayName}</h1>
          <p className="text-xs font-mono text-subtle-ui mb-6 break-all">{fullId}</p>

          {/* Overview */}
          <Section title={t('images.overview')}>
            <KeyValue label={t('images.architecture')} value={asString(safeGet(inspect, 'Architecture'))} />
            <KeyValue label={t('images.os')} value={asString(safeGet(inspect, 'Os'))} />
            <KeyValue label="Created" value={asString(safeGet(inspect, 'Created'))} />
            <KeyValue label={t('images.size')} value={formatBytes(typeof inspect['Size'] === 'number' ? inspect['Size'] : 0)} />
            <KeyValue label={t('images.dockerVersion')} value={asString(safeGet(inspect, 'DockerVersion'))} />
            <KeyValue label={t('images.author')} value={asString(safeGet(inspect, 'Author'))} />
          </Section>

          {/* Config */}
          {config != null && (
            <Section title={t('images.config')}>
              <KeyValue label={t('images.entrypoint')} value={asStringArray(safeGet(config, 'Entrypoint')).join(' ')} />
              <KeyValue label={t('images.cmd')} value={asStringArray(safeGet(config, 'Cmd')).join(' ')} />
              <KeyValue label={t('images.workingDir')} value={asString(safeGet(config, 'WorkingDir'))} />
              <KeyValue
                label={t('images.exposedPorts')}
                value={Object.keys(asRecord(safeGet(config, 'ExposedPorts'))).join(', ')}
              />
              <KeyValue
                label={t('images.volumes')}
                value={Object.keys(asRecord(safeGet(config, 'Volumes'))).join(', ')}
              />
            </Section>
          )}

          {/* Environment */}
          {config != null && asStringArray(safeGet(config, 'Env')).length > 0 && (
            <Section title={t('images.environment')}>
              <div className="max-h-80 overflow-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-[var(--border)]">
                      <th className="text-left py-1.5 pr-4 text-subtle-ui font-semibold uppercase tracking-wider">Key</th>
                      <th className="text-left py-1.5 text-subtle-ui font-semibold uppercase tracking-wider">Value</th>
                    </tr>
                  </thead>
                  <tbody>
                    {asStringArray(safeGet(config, 'Env')).map((env, i) => {
                      const eqIdx = env.indexOf('=');
                      const key = eqIdx >= 0 ? env.slice(0, eqIdx) : env;
                      const val = eqIdx >= 0 ? env.slice(eqIdx + 1) : '';
                      return (
                        <tr key={i} className="border-b border-[var(--border)] last:border-0">
                          <td className="py-1.5 pr-4 font-mono font-medium text-main">{key}</td>
                          <td className="py-1.5 font-mono text-subtle-ui break-all">{val}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </Section>
          )}

          {/* Labels */}
          {config != null && Object.keys(asRecord(safeGet(config, 'Labels'))).length > 0 && (
            <Section title={t('images.labels')}>
              <div className="max-h-60 overflow-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-[var(--border)]">
                      <th className="text-left py-1.5 pr-4 text-subtle-ui font-semibold uppercase tracking-wider">Key</th>
                      <th className="text-left py-1.5 text-subtle-ui font-semibold uppercase tracking-wider">Value</th>
                    </tr>
                  </thead>
                  <tbody>
                    {Object.entries(asRecord(safeGet(config, 'Labels'))).map(([key, val]) => (
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

          {/* Used by Services */}
          <Section title={t('images.usedBy')}>
            {containers.length === 0 ? (
              <p className="text-xs text-subtle-ui">{t('images.noContainers')}</p>
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

          {/* Layer History */}
          {history.length > 0 && (
            <Section title={t('images.layers')}>
              <div className="max-h-96 overflow-auto">
                {history.map((layer, i) => (
                  <div
                    key={i}
                    className={`py-2 px-2 -mx-2 rounded text-xs ${
                      layer.empty_layer === true ? 'opacity-50' : ''
                    } ${i > 0 ? 'border-t border-[var(--border)]' : ''}`}
                  >
                    <div className="flex items-center gap-2 mb-0.5">
                      <span className="font-mono text-subtle-ui">#{history.length - i}</span>
                      {layer.size > 0 && (
                        <span className="text-[10px] px-1.5 py-0.5 rounded bg-blue-500/10 text-blue-600 dark:text-blue-400 font-semibold">
                          {formatBytes(layer.size)}
                        </span>
                      )}
                    </div>
                    <p className="font-mono text-subtle-ui leading-relaxed break-all">
                      {truncateCommand(layer.created_by)}
                    </p>
                  </div>
                ))}
              </div>
            </Section>
          )}
        </>
      )}
    </div>
  );
}
