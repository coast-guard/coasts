import { useMemo, useState, useCallback } from 'react';
import { useParams, Link } from 'react-router';
import { useTranslation } from 'react-i18next';
import { useSharedServices, useSharedStopMutation, useSharedStartMutation, useSharedRestartMutation } from '../api/hooks';
import { ApiError } from '../api/client';
import Breadcrumb from '../components/Breadcrumb';
import TabBar, { type TabDef } from '../components/TabBar';
import Modal from '../components/Modal';

import HostServiceExecTab from './HostServiceExecTab';
import HostServiceLogsTab from './HostServiceLogsTab';
import HostServiceStatsTab from './HostServiceStatsTab';
import HostServiceInspectTab from './HostServiceInspectTab';

type TabId = 'exec' | 'inspect' | 'logs' | 'stats';
const VALID_TABS = new Set<string>(['exec', 'inspect', 'logs', 'stats']);

function parseTab(raw: string | undefined): TabId {
  if (raw != null && VALID_TABS.has(raw)) return raw as TabId;
  return 'exec';
}

export default function HostServiceDetailPage() {
  const { t, i18n } = useTranslation();
  const params = useParams<{
    project: string;
    service: string;
    tab: string;
  }>();
  const project = params.project ?? '';
  const service = params.service ?? '';
  const activeTab = parseTab(params.tab);

  const { data } = useSharedServices(project);
  const svcInfo = data?.services.find((s) => s.name === service);
  const isRunning = svcInfo != null && svcInfo.status === 'running';

  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const stopMut = useSharedStopMutation();
  const startMut = useSharedStartMutation();
  const restartMut = useSharedRestartMutation();

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

  const vars = { project, service };

  const basePath = `/project/${project}/host-services/${encodeURIComponent(service)}`;
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
          { label: t('shared.title'), to: `/project/${project}/shared-services` },
          { label: service },
        ]}
      />

      <div className="flex items-center gap-3 mb-2">
        <h1 className="text-2xl font-bold text-main">{service}</h1>
        <span className="px-2 py-0.5 rounded text-[10px] font-semibold uppercase tracking-wider bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20">
          {t('shared.hostBadge')}
        </span>
        {svcInfo != null && (
          <span className={`inline-flex items-center gap-1.5 text-xs px-2 py-0.5 rounded-full border ${
            isRunning
              ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/30'
              : 'bg-slate-500/10 text-subtle-ui border-[var(--border)]'
          }`}>
            <span className={`h-1.5 w-1.5 rounded-full ${isRunning ? 'bg-emerald-500' : 'bg-slate-400'}`} />
            {svcInfo.status}
          </span>
        )}
      </div>

      {svcInfo?.image != null && (
        <p className="text-xs font-mono text-subtle-ui mb-3">
          <Link
            to={`/project/${project}/host-images/${encodeURIComponent(svcInfo.image)}`}
            className="text-[var(--primary)] hover:underline"
          >
            {svcInfo.image}
          </Link>
        </p>
      )}

      {svcInfo?.ports != null && svcInfo.ports.length > 0 && (
        <div className="mb-4 flex flex-wrap items-center gap-1.5">
          {svcInfo.ports.split(',').map((p) => p.trim()).filter((p) => p.length > 0).map((port, i) => (
            <span key={i} className="px-2 py-0.5 rounded text-[10px] font-mono text-subtle-ui bg-white/10 dark:bg-white/5 border border-[var(--border)]">
              {port}
            </span>
          ))}
        </div>
      )}

      <p className="text-sm text-subtle-ui mb-4">
        {t('shared.subtitle')}
      </p>

      <div className="flex items-center gap-2 mb-6">
        {isRunning ? (
          <button
            className="btn btn-outline"
            onClick={() => void act(() => stopMut.mutateAsync(vars))}
          >
            {t('action.stop')}
          </button>
        ) : (
          <button
            className="btn btn-primary"
            onClick={() => void act(() => startMut.mutateAsync(vars))}
          >
            {t('action.start')}
          </button>
        )}
        <button
          className="btn btn-outline"
          onClick={() => void act(() => restartMut.mutateAsync(vars))}
        >
          {t('shared.restart')}
        </button>
      </div>

      <TabBar tabs={tabs} active={activeTab} />
      <div className="mt-1">
        {activeTab === 'exec' && (
          <HostServiceExecTab project={project} service={service} />
        )}
        {activeTab === 'inspect' && (
          <HostServiceInspectTab project={project} service={service} />
        )}
        {activeTab === 'logs' && (
          <HostServiceLogsTab project={project} service={service} />
        )}
        {activeTab === 'stats' && (
          <HostServiceStatsTab project={project} service={service} />
        )}
      </div>

      <Modal open={errorMsg != null} title={t('error.title')} onClose={() => setErrorMsg(null)}>
        <p className="text-rose-600 dark:text-rose-400">{errorMsg}</p>
      </Modal>
    </div>
  );
}
