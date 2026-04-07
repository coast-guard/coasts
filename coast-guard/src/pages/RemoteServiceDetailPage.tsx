import { useMemo, useState, useCallback, useEffect } from 'react';
import { useParams } from 'react-router';
import { useTranslation } from 'react-i18next';
import { ArrowClockwise, CloudArrowUp } from '@phosphor-icons/react';
import { projectName, instanceName } from '../types/branded';
import { useServices, usePorts, useInstances, useServiceInspect, useServiceStopMutation, useServiceStartMutation, useServiceRestartMutation, useRemotesLs } from '../api/hooks';
import { api } from '../api/endpoints';
import { ApiError } from '../api/client';
import Breadcrumb from '../components/Breadcrumb';
import TabBar, { type TabDef } from '../components/TabBar';
import Modal from '../components/Modal';
import { serviceOpKey, useServiceOperations, isInProgress } from '../providers/ServiceOperationsProvider';

import ServiceExecTab from './ServiceExecTab';
import ServiceLogsTab from './ServiceLogsTab';
import ServiceStatsTab from './ServiceStatsTab';
import ServiceInspectTab from './ServiceInspectTab';

type TabId = 'exec' | 'inspect' | 'logs' | 'stats';
const VALID_TABS = new Set<string>(['exec', 'inspect', 'logs', 'stats']);

function parseTab(raw: string | undefined): TabId {
  if (raw != null && VALID_TABS.has(raw)) return raw as TabId;
  return 'exec';
}

export default function RemoteServiceDetailPage() {
  const { t, i18n } = useTranslation();
  const params = useParams<{
    project: string;
    name: string;
    service: string;
    tab: string;
  }>();
  const project = projectName(params.project ?? '');
  const name = instanceName(params.name ?? '');
  const service = params.service ?? '';
  const activeTab = parseTab(params.tab);

  const { data, error: servicesError } = useServices(project, name);
  const svcInfo = data?.services.find((s) => s.name === service);
  const { data: portsData } = usePorts(project, name);
  const { data: instancesData } = useInstances(project);
  const { data: remotesData } = useRemotesLs();
  const instance = instancesData?.instances.find((i) => (i.name as string) === (name as string));
  const checkedOut = instance?.checked_out ?? false;

  const remoteHost = instance?.remote_host;
  const remoteName = useMemo(() => {
    if (!remoteHost) return null;
    const allRemotes = remotesData?.remotes ?? [];
    return allRemotes.find(
      (r: { name: string; host: string }) => r.name === remoteHost || r.host === remoteHost,
    )?.name ?? remoteHost;
  }, [remoteHost, remotesData]);
  const remoteReachable = remoteHost ? !servicesError : true;

  const { data: inspectData } = useServiceInspect(project as string, name as string, service);

  const portMapping = useMemo(() => {
    if (portsData?.ports == null) return undefined;
    return portsData.ports.find((p) => p.logical_name === service);
  }, [portsData, service]);

  const volumeNames = useMemo(() => {
    if (inspectData == null) return [];
    const inspect = Array.isArray(inspectData) && inspectData.length > 0
      ? inspectData[0] as Record<string, unknown>
      : null;
    if (inspect == null) return [];
    const mounts = inspect['Mounts'];
    if (!Array.isArray(mounts)) return [];
    const names: string[] = [];
    for (const m of mounts) {
      const mount = m as Record<string, unknown>;
      if (mount['Type'] === 'volume' && typeof mount['Name'] === 'string') {
        names.push(mount['Name'] as string);
      }
    }
    return names;
  }, [inspectData]);

  const [urlTemplate, setUrlTemplate] = useState<string>('http://localhost:<port>');
  useEffect(() => {
    void (async () => {
      const val = await api.getSetting(`port_url:${project}:${service}`);
      if (val != null) setUrlTemplate(val);
    })();
  }, [project, service]);

  const { operations } = useServiceOperations();
  const opKey = serviceOpKey(project as string, name as string, service);
  const currentOp = operations.get(opKey);
  const operationInProgress = isInProgress(currentOp);

  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const stopMut = useServiceStopMutation();
  const startMut = useServiceStartMutation();
  const restartMut = useServiceRestartMutation();

  const act = useCallback(
    async (fn: () => Promise<unknown>) => {
      try {
        await fn();
      } catch (e) {
        setErrorMsg(e instanceof ApiError ? e.body.error : String(e));
      }
    },
    [],
  );

  const vars = { project: project as string, name: name as string, service };
  const isRunning = svcInfo != null && svcInfo.status === 'running';

  const basePath = `/remote-instance/${project}/${name}/services/${encodeURIComponent(service)}`;
  const tabs: readonly TabDef<TabId>[] = useMemo(
    () => [
      { id: 'exec' as const, label: t('tab.exec'), to: `${basePath}/exec` },
      { id: 'inspect' as const, label: t('tab.inspect'), to: `${basePath}/inspect` },
      { id: 'logs' as const, label: t('tab.logs'), to: `${basePath}/logs` },
      { id: 'stats' as const, label: t('tab.stats'), to: `${basePath}/stats` },
    ],
    [basePath, t, i18n.language],
  );

  return (
    <div className="page-shell">
      <Breadcrumb
        items={[
          { label: t('nav.projects'), to: '/' },
          { label: project, to: `/project/${project}` },
          { label: name, to: `/remote-instance/${project}/${name}` },
          { label: t('tab.services'), to: `/remote-instance/${project}/${name}/services` },
          { label: service },
        ]}
      />

      <div className="flex items-center gap-3 mb-2">
        <h1 className="text-2xl font-bold text-main">{service}</h1>
        {remoteName && (
          <span className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 text-xs font-medium rounded-full shrink-0 ${
            remoteReachable
              ? 'bg-indigo-500/12 border border-indigo-500/30 text-indigo-700 dark:text-indigo-300'
              : 'bg-red-500/12 border border-red-500/30 text-red-700 dark:text-red-300'
          }`}>
            <CloudArrowUp size={12} weight="fill" />
            {remoteName}
          </span>
        )}
        {svcInfo != null && (
          <span className={`inline-flex items-center gap-1.5 text-xs px-2 py-0.5 rounded-full border ${
            !remoteReachable
              ? 'bg-red-500/10 text-red-600 dark:text-red-400 border-red-500/30'
              : isRunning
                ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/30'
                : 'bg-slate-500/10 text-subtle-ui border-[var(--border)]'
          }`}>
            <span className={`h-1.5 w-1.5 rounded-full ${!remoteReachable ? 'bg-red-500' : isRunning ? 'bg-emerald-500' : 'bg-slate-400'}`} />
            {!remoteReachable ? t('status.remoteDown', 'Remote Down') : svcInfo.status}
          </span>
        )}
      </div>

      {svcInfo != null && svcInfo.image.length > 0 && (
        <p className="text-xs font-mono mb-3">
          <span className="text-subtle-ui">{svcInfo.image}</span>
        </p>
      )}

      {(svcInfo != null && svcInfo.ports.length > 0 || portMapping != null) && (
        <div className="mb-4 flex flex-col gap-2">
          {portMapping != null && (
            <div className="flex flex-wrap items-center gap-2">
              {checkedOut ? (
                <a
                  href={urlTemplate.replace('<port>', String(portMapping.canonical_port))}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center px-2.5 py-1 rounded-md text-xs font-mono bg-blue-500/10 text-[var(--primary)] border border-blue-500/20 hover:bg-blue-500/20 transition-colors"
                >
                  {urlTemplate.replace('<port>', String(portMapping.canonical_port))}
                </a>
              ) : (
                <span className="inline-flex items-center px-2.5 py-1 rounded-md text-xs font-mono bg-slate-500/10 text-subtle-ui border border-[var(--border)]">
                  {urlTemplate.replace('<port>', String(portMapping.canonical_port))}
                </span>
              )}
              {(() => {
                let dynUrl = urlTemplate.replace('<port>', String(portMapping.dynamic_port));
                if (portsData?.subdomain_host != null) {
                  dynUrl = dynUrl.replace('localhost:', `${portsData.subdomain_host}:`);
                }
                return (
                  <a
                    href={dynUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center px-2.5 py-1 rounded-md text-xs font-mono bg-blue-500/10 text-[var(--primary)] border border-blue-500/20 hover:bg-blue-500/20 transition-colors"
                  >
                    {dynUrl}
                  </a>
                );
              })()}
            </div>
          )}
          {svcInfo != null && svcInfo.ports.length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5">
              {svcInfo.ports.split(',').map((p) => p.trim()).filter((p) => p.length > 0).map((port, i) => (
                <span key={i} className="px-2 py-0.5 rounded text-[10px] font-mono text-subtle-ui bg-white/10 dark:bg-white/5 border border-[var(--border)]">
                  {port}
                </span>
              ))}
            </div>
          )}
        </div>
      )}

      {volumeNames.length > 0 && (
        <div className="mb-4 flex flex-wrap items-center gap-1.5">
          <span className="text-[10px] font-semibold text-subtle-ui uppercase tracking-wider mr-1">{t('tab.volumes')}</span>
          {volumeNames.map((vol) => (
            <span
              key={vol}
              className="inline-flex items-center px-2 py-0.5 rounded text-[10px] font-mono text-subtle-ui bg-purple-500/10 border border-purple-500/20"
            >
              {vol}
            </span>
          ))}
        </div>
      )}

      <p className="text-sm text-subtle-ui mb-4">
        {t('service.subtitle', { instance: name as string })}
      </p>

      {currentOp != null && (
        <div className={`mb-4 px-4 py-2.5 rounded-lg flex items-center gap-2 text-sm ${
          currentOp.status === 'error'
            ? 'bg-rose-500/10 border border-rose-500/30 text-rose-600 dark:text-rose-400'
            : operationInProgress
              ? 'bg-blue-500/10 border border-blue-500/30 text-blue-600 dark:text-blue-400'
              : 'bg-emerald-500/10 border border-emerald-500/30 text-emerald-600 dark:text-emerald-400'
        }`}>
          {operationInProgress && <ArrowClockwise size={16} className="animate-spin" />}
          <span className="font-medium capitalize">
            {currentOp.status === 'error'
              ? `${t('service.operationError')}: ${currentOp.error ?? ''}`
              : t(`service.${currentOp.status}`)
            }
          </span>
        </div>
      )}

      <div className="flex items-center gap-2 mb-6">
        {isRunning ? (
          <button
            className="btn btn-outline"
            disabled={operationInProgress}
            onClick={() => void act(() => stopMut.mutateAsync(vars))}
          >
            {t('action.stop')}
          </button>
        ) : (
          <button
            className="btn btn-primary"
            disabled={operationInProgress}
            onClick={() => void act(() => startMut.mutateAsync(vars))}
          >
            {t('action.start')}
          </button>
        )}
        <button
          className="btn btn-outline"
          disabled={operationInProgress}
          onClick={() => void act(() => restartMut.mutateAsync(vars))}
        >
          {t('service.restart')}
        </button>
      </div>

      <TabBar tabs={tabs} active={activeTab} />
      <div className="mt-1">
        {activeTab === 'exec' && (
          <ServiceExecTab project={project} name={name} service={service} />
        )}
        {activeTab === 'inspect' && (
          <ServiceInspectTab project={project} name={name} service={service} />
        )}
        {activeTab === 'logs' && (
          <ServiceLogsTab project={project} name={name} service={service} />
        )}
        {activeTab === 'stats' && (
          <ServiceStatsTab project={project} name={name} service={service} />
        )}
      </div>

      <Modal open={errorMsg != null} title={t('error.title')} onClose={() => setErrorMsg(null)}>
        <p className="text-rose-600 dark:text-rose-400">{errorMsg}</p>
      </Modal>
    </div>
  );
}
