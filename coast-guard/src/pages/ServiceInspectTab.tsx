import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router';
import type { ProjectName, InstanceName } from '../types/branded';
import { useServiceInspect } from '../api/hooks';
import Section from '../components/Section';
import KeyValue from '../components/KeyValue';

interface Props {
  readonly project: ProjectName;
  readonly name: InstanceName;
  readonly service: string;
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
  if (typeof val === 'number' || typeof val === 'boolean') return String(val);
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

interface MountInfo {
  readonly Type: string;
  readonly Source: string;
  readonly Destination: string;
  readonly Mode: string;
  readonly RW: boolean;
}

function asMounts(val: unknown): readonly MountInfo[] {
  if (!Array.isArray(val)) return [];
  return val as MountInfo[];
}

export default function ServiceInspectTab({ project, name, service }: Props) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { data, isLoading, error } = useServiceInspect(project as string, name as string, service);

  const inspectArr = (data as Record<string, unknown> | undefined)?.['inspect'];
  const inspect = Array.isArray(inspectArr) && inspectArr.length > 0 ? inspectArr[0] as Record<string, unknown> : null;

  if (isLoading) return <p className="text-sm text-subtle-ui py-4">{t('inspect.loading')}</p>;
  if (error != null) return <p className="text-sm text-rose-500 py-4">{String(error)}</p>;
  if (inspect == null) return <p className="text-sm text-subtle-ui py-4">{t('inspect.noData')}</p>;

  const state = safeGet(inspect, 'State') as Record<string, unknown> | undefined;
  const config = safeGet(inspect, 'Config') as Record<string, unknown> | undefined;
  const networkSettings = safeGet(inspect, 'NetworkSettings') as Record<string, unknown> | undefined;
  const mounts = asMounts(safeGet(inspect, 'Mounts'));
  const hostConfig = safeGet(inspect, 'HostConfig') as Record<string, unknown> | undefined;

  return (
    <div className="flex flex-col gap-0">
      {state != null && (
        <Section title={t('inspect.state')}>
          <KeyValue label={t('inspect.status')} value={asString(state['Status'])} />
          <KeyValue label={t('inspect.running')} value={asString(state['Running'])} />
          <KeyValue label={t('inspect.paused')} value={asString(state['Paused'])} />
          <KeyValue label={t('inspect.restarting')} value={asString(state['Restarting'])} />
          <KeyValue label={t('inspect.oomKilled')} value={asString(state['OOMKilled'])} />
          <KeyValue label={t('inspect.pid')} value={asString(state['Pid'])} />
          <KeyValue label={t('inspect.exitCode')} value={asString(state['ExitCode'])} />
          <KeyValue label={t('inspect.startedAt')} value={asString(state['StartedAt'])} />
          <KeyValue label={t('inspect.finishedAt')} value={asString(state['FinishedAt'])} />
        </Section>
      )}

      {config != null && (
        <Section title={t('inspect.config')}>
          {asString(safeGet(config, 'Image')).length > 0 && (
            <div className="flex gap-3 py-1.5 border-b border-[var(--border)] last:border-0">
              <span className="text-xs text-subtle-ui w-40 shrink-0 font-medium">{t('inspect.image')}</span>
              <span
                className="text-xs font-mono cursor-pointer text-blue-600 dark:text-blue-400 hover:underline transition-colors break-all"
                onClick={() => navigate(`/instance/${project}/${name}/images/${encodeURIComponent(asString(safeGet(config, 'Image')))}`)}
              >
                {asString(safeGet(config, 'Image'))}
              </span>
            </div>
          )}
          <KeyValue label={t('inspect.hostname')} value={asString(safeGet(config, 'Hostname'))} />
          <KeyValue label={t('inspect.cmd')} value={asStringArray(safeGet(config, 'Cmd')).join(' ')} />
          <KeyValue label={t('inspect.entrypoint')} value={asStringArray(safeGet(config, 'Entrypoint')).join(' ')} />
          <KeyValue label={t('inspect.workingDir')} value={asString(safeGet(config, 'WorkingDir'))} />
          <KeyValue label={t('inspect.exposedPorts')} value={Object.keys(asRecord(safeGet(config, 'ExposedPorts'))).join(', ')} />
        </Section>
      )}

      {networkSettings != null && (
        <Section title={t('inspect.network')}>
          <KeyValue label={t('inspect.networkMode')} value={asString(safeGet(hostConfig, 'NetworkMode'))} />
          <KeyValue label={t('inspect.ipAddress')} value={asString(safeGet(networkSettings, 'IPAddress'))} />
          <KeyValue label={t('inspect.gateway')} value={asString(safeGet(networkSettings, 'Gateway'))} />
          <KeyValue label={t('inspect.macAddress')} value={asString(safeGet(networkSettings, 'MacAddress'))} />
          {(() => {
            const networks = safeGet(networkSettings, 'Networks') as Record<string, unknown> | undefined;
            if (networks == null) return null;
            return Object.entries(networks).map(([netName, netVal]) => {
              const net = netVal as Record<string, unknown>;
              return (
                <div key={netName} className="mt-2 pt-2 border-t border-[var(--border)]">
                  <span className="text-xs font-semibold text-main">{netName}</span>
                  <KeyValue label={t('inspect.ipAddress')} value={asString(safeGet(net, 'IPAddress'))} />
                  <KeyValue label={t('inspect.gateway')} value={asString(safeGet(net, 'Gateway'))} />
                  <KeyValue label={t('inspect.macAddress')} value={asString(safeGet(net, 'MacAddress'))} />
                  <KeyValue label={t('inspect.networkId')} value={asString(safeGet(net, 'NetworkID'))} />
                </div>
              );
            });
          })()}
        </Section>
      )}

      {mounts.length > 0 && (
        <Section title={t('inspect.mounts')}>
          <div className="max-h-60 overflow-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b border-[var(--border)]">
                  <th className="text-left py-1.5 pr-3 text-subtle-ui font-semibold uppercase tracking-wider">{t('inspect.mountType')}</th>
                  <th className="text-left py-1.5 pr-3 text-subtle-ui font-semibold uppercase tracking-wider">{t('inspect.mountSource')}</th>
                  <th className="text-left py-1.5 pr-3 text-subtle-ui font-semibold uppercase tracking-wider">{t('inspect.mountDestination')}</th>
                  <th className="text-left py-1.5 pr-3 text-subtle-ui font-semibold uppercase tracking-wider">{t('inspect.mountMode')}</th>
                  <th className="text-left py-1.5 text-subtle-ui font-semibold uppercase tracking-wider">{t('inspect.mountRw')}</th>
                </tr>
              </thead>
              <tbody>
                {mounts.map((m, i) => (
                  <tr key={i} className="border-b border-[var(--border)] last:border-0">
                    <td className="py-1.5 pr-3 font-mono text-main">{m.Type}</td>
                    <td className="py-1.5 pr-3 font-mono text-subtle-ui break-all">{m.Source}</td>
                    <td className="py-1.5 pr-3 font-mono text-main">{m.Destination}</td>
                    <td className="py-1.5 pr-3 font-mono text-subtle-ui">{m.Mode || '—'}</td>
                    <td className="py-1.5 font-mono text-main">{m.RW ? t('inspect.yes') : t('inspect.no')}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Section>
      )}

      {config != null && asStringArray(safeGet(config, 'Env')).length > 0 && (
        <Section title={t('inspect.environment')}>
          <div className="max-h-80 overflow-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b border-[var(--border)]">
                  <th className="text-left py-1.5 pr-4 text-subtle-ui font-semibold uppercase tracking-wider">{t('inspect.envKey')}</th>
                  <th className="text-left py-1.5 text-subtle-ui font-semibold uppercase tracking-wider">{t('inspect.envValue')}</th>
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
        <Section title={t('inspect.labels')}>
          <div className="max-h-60 overflow-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b border-[var(--border)]">
                  <th className="text-left py-1.5 pr-4 text-subtle-ui font-semibold uppercase tracking-wider">{t('inspect.envKey')}</th>
                  <th className="text-left py-1.5 text-subtle-ui font-semibold uppercase tracking-wider">{t('inspect.envValue')}</th>
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
    </div>
  );
}
