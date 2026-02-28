import { useParams } from 'react-router';
import { useTranslation } from 'react-i18next';
import { useHostImageInspect } from '../api/hooks';
import Breadcrumb from '../components/Breadcrumb';
import Section from '../components/Section';
import KeyValue from '../components/KeyValue';

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

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export default function HostImageDetailPage() {
  const { t } = useTranslation();
  const params = useParams<{
    project: string;
    imageId: string;
  }>();
  const project = params.project ?? '';
  const imageId = decodeURIComponent(params.imageId ?? '');

  const { data, isLoading, error } = useHostImageInspect(project, imageId);

  const inspect = data != null && typeof data === 'object' && !Array.isArray(data) ? data as Record<string, unknown> : null;

  const repoTags = inspect != null ? asStringArray(safeGet(inspect, 'RepoTags')) : [];
  const displayName = repoTags.length > 0 ? repoTags[0]! : imageId;
  const fullId = asString(safeGet(inspect, 'Id'));
  const config = inspect != null ? (safeGet(inspect, 'ContainerConfig') ?? safeGet(inspect, 'Config')) as Record<string, unknown> | undefined : undefined;

  return (
    <div className="page-shell">
      <Breadcrumb
        items={[
          { label: t('nav.projects'), to: '/' },
          { label: project, to: `/project/${project}` },
          { label: t('breadcrumb.hostImages') },
          { label: displayName || imageId },
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
          <div className="flex items-center gap-3 mb-1">
            <h1 className="text-2xl font-bold text-main">{displayName}</h1>
            <span className="px-2 py-0.5 rounded text-[10px] font-semibold uppercase tracking-wider bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20">
              {t('shared.hostBadge')}
            </span>
          </div>
          {fullId.length > 0 && (
            <p className="text-xs font-mono text-subtle-ui mb-6 break-all">{fullId}</p>
          )}

          <Section title={t('images.overview')}>
            <KeyValue label={t('images.architecture')} value={asString(safeGet(inspect, 'Architecture'))} />
            <KeyValue label={t('images.os')} value={asString(safeGet(inspect, 'Os'))} />
            <KeyValue label="Created" value={asString(safeGet(inspect, 'Created'))} />
            <KeyValue label={t('images.size')} value={formatBytes(typeof inspect['Size'] === 'number' ? inspect['Size'] : 0)} />
            <KeyValue label={t('images.dockerVersion')} value={asString(safeGet(inspect, 'DockerVersion'))} />
            <KeyValue label={t('images.author')} value={asString(safeGet(inspect, 'Author'))} />
          </Section>

          {config != null && (
            <Section title={t('images.config')}>
              <KeyValue label={t('images.entrypoint')} value={asStringArray(safeGet(config, 'Entrypoint')).join(' ')} />
              <KeyValue label={t('images.cmd')} value={asStringArray(safeGet(config, 'Cmd')).join(' ')} />
              <KeyValue label={t('images.workingDir')} value={asString(safeGet(config, 'WorkingDir'))} />
              <KeyValue
                label={t('images.exposedPorts')}
                value={Object.keys(asRecord(safeGet(config, 'ExposedPorts'))).join(', ')}
              />
            </Section>
          )}

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
        </>
      )}
    </div>
  );
}
