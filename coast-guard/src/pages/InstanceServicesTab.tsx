import { useMemo, useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router';
import { ArrowClockwise, Warning } from '@phosphor-icons/react';
import type { ProjectName, InstanceName } from '../types/branded';
import type { ServiceStatus, PortMapping } from '../types/api';
import { useServices, usePorts, useServiceStopMutation, useServiceStartMutation, useServiceRestartMutation, useServiceRmMutation } from '../api/hooks';
import { api } from '../api/endpoints';
import { ApiError } from '../api/client';
import DataTable, { type Column } from '../components/DataTable';
import Toolbar, { type ToolbarAction } from '../components/Toolbar';
import Modal from '../components/Modal';
import { serviceOpKey, useServiceOperations, isInProgress } from '../providers/ServiceOperationsProvider';

interface Props {
  readonly project: ProjectName;
  readonly name: InstanceName;
  readonly checkedOut: boolean;
}

const DEFAULT_TEMPLATE = 'http://localhost:<port>';

function resolvePortUrl(template: string, port: number): string {
  return template.replace('<port>', String(port));
}

function applySubdomainHost(url: string, subdomainHost: string | null | undefined): string {
  if (subdomainHost == null) return url;
  return url.replace('localhost:', `${subdomainHost}:`);
}

function splitPorts(portsStr: string): readonly string[] {
  if (portsStr.length === 0) return [];
  return portsStr.split(',').map((p) => p.trim()).filter((p) => p.length > 0);
}

export default function InstanceServicesTab({ project, name, checkedOut }: Props) {
  const { t, i18n } = useTranslation();
  const { data, isLoading, error } = useServices(project, name);
  const { data: portsData } = usePorts(project, name);
  const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(new Set());
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [templates, setTemplates] = useState<Record<string, string>>({});
  const { operations } = useServiceOperations();

  const stopMut = useServiceStopMutation();
  const startMut = useServiceStartMutation();
  const restartMut = useServiceRestartMutation();
  const rmMut = useServiceRmMutation();

  const services = data?.services ?? [];

  const portMap = useMemo(() => {
    const map: Record<string, PortMapping> = {};
    if (portsData?.ports != null) {
      for (const p of portsData.ports) {
        map[p.logical_name] = p;
      }
    }
    return map;
  }, [portsData]);

  useEffect(() => {
    if (services.length === 0) return;
    let cancelled = false;
    void (async () => {
      const result: Record<string, string> = {};
      for (const svc of services) {
        const val = await api.getSetting(`port_url:${project}:${svc.name}`);
        if (cancelled) return;
        if (val != null) result[svc.name] = val;
      }
      if (!cancelled) setTemplates(result);
    })();
    return () => { cancelled = true; };
  }, [services.length, project]);

  const selectedNames = useMemo(
    () => services.filter((s) => selectedIds.has(s.name)).map((s) => s.name),
    [services, selectedIds],
  );

  const batchAction = useCallback(
    async (action: (vars: { project: string; name: string; service: string }) => Promise<unknown>) => {
      const errors: string[] = [];
      for (const svc of selectedNames) {
        try {
          await action({ project: project as string, name: name as string, service: svc });
        } catch (e) {
          errors.push(`${svc}: ${e instanceof ApiError ? e.body.error : String(e)}`);
        }
      }
      setSelectedIds(new Set());
      if (errors.length > 0) setErrorMsg(errors.join('\n'));
    },
    [selectedNames, project, name],
  );

  const toolbarActions: readonly ToolbarAction[] = useMemo(
    () => [
      { label: t('action.stop'), variant: 'outline' as const, onClick: () => void batchAction((v) => stopMut.mutateAsync(v)) },
      { label: t('action.start'), variant: 'outline' as const, onClick: () => void batchAction((v) => startMut.mutateAsync(v)) },
      { label: t('service.restart'), variant: 'outline' as const, onClick: () => void batchAction((v) => restartMut.mutateAsync(v)) },
      { label: t('action.remove'), variant: 'danger' as const, onClick: () => void batchAction((v) => rmMut.mutateAsync(v)) },
    ],
    [batchAction, stopMut, startMut, restartMut, rmMut, t, i18n.language],
  );

  const columns: readonly Column<ServiceStatus>[] = useMemo(
    () => [
      {
        key: 'name',
        header: t('col.service'),
        headerClassName: 'w-[22%]',
        className: 'w-[22%]',
        render: (r) => {
          const isBare = r.kind === 'bare';
          const isDown = r.status !== 'running';
          return (
            <span className="inline-flex items-center gap-2">
              {isBare ? (
                <span className="font-medium text-main">{r.name}</span>
              ) : (
                <Link
                  to={`/instance/${project}/${name}/services/${encodeURIComponent(r.name)}`}
                  className="font-medium text-[var(--primary)] hover:underline"
                >
                  {r.name}
                </Link>
              )}
              {isBare && (
                <span className="inline-block px-1.5 py-0.5 rounded text-[10px] font-semibold bg-amber-500/15 text-amber-600 dark:text-amber-400">
                  bare
                </span>
              )}
              {isDown && (
                <Warning size={14} weight="fill" className="text-amber-500 shrink-0" />
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
          const op = operations.get(serviceOpKey(project as string, name as string, r.name));
          if (op != null && isInProgress(op)) {
            return (
              <span className="inline-flex items-center gap-1.5 text-xs">
                <ArrowClockwise size={14} className="animate-spin text-[var(--primary)]" />
                <span className="text-[var(--primary)] font-medium capitalize">{t(`service.${op.status}`)}</span>
              </span>
            );
          }
          if (op != null && op.status === 'error') {
            return <span className="text-xs text-rose-500 font-medium">{t('service.operationError')}</span>;
          }
          const isRunning = r.status === 'running';
          return (
            <span className={`inline-flex items-center gap-1.5 text-xs ${isRunning ? 'text-emerald-600 dark:text-emerald-400' : 'text-subtle-ui'}`}>
              <span className={`h-1.5 w-1.5 rounded-full ${isRunning ? 'bg-emerald-500' : 'bg-slate-400'}`} />
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
          <Link
            to={`/instance/${project}/${name}/images/${encodeURIComponent(r.image)}`}
            className="font-mono text-xs text-[var(--primary)] hover:underline truncate max-w-[200px] inline-block"
            title={r.image}
            onClick={(e) => e.stopPropagation()}
          >
            {r.image}
          </Link>
        ),
      },
      {
        key: 'ports',
        header: t('col.ports'),
        render: (r) => {
          const rawPorts = splitPorts(r.ports);
          const mapping = portMap[r.name];
          const template = templates[r.name] ?? DEFAULT_TEMPLATE;

          if (rawPorts.length === 0 && mapping == null) {
            return <span className="text-subtle-ui text-xs">—</span>;
          }

          return (
            <div className="text-xs font-mono leading-5">
              {rawPorts.length > 0 && (
                <div className="text-subtle-ui">
                  {rawPorts.map((port, i) => (
                    <span key={i}>
                      {port}
                      {i < rawPorts.length - 1 && <br />}
                    </span>
                  ))}
                </div>
              )}
              {mapping != null && (
                <div className={`flex items-center gap-2 ${rawPorts.length > 0 ? 'mt-1.5 pt-1.5 border-t border-[var(--border)]' : ''}`}>
                  {checkedOut ? (
                    <a
                      href={resolvePortUrl(template, mapping.canonical_port)}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-[var(--primary)] hover:underline"
                      onClick={(e) => e.stopPropagation()}
                    >
                      :{mapping.canonical_port}
                    </a>
                  ) : (
                    <span className="text-subtle-ui">:{mapping.canonical_port}</span>
                  )}
                  <span className="text-subtle-ui">/</span>
                  <a
                    href={applySubdomainHost(resolvePortUrl(template, mapping.dynamic_port), portsData?.subdomain_host)}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-[var(--primary)] hover:underline"
                    onClick={(e) => e.stopPropagation()}
                  >
                    :{mapping.dynamic_port}
                  </a>
                </div>
              )}
            </div>
          );
        },
      },
    ],
    [t, i18n.language, project, name, operations, templates, portMap, checkedOut],
  );

  const downSvcs = useMemo(
    () => services.filter((s) => s.status !== 'running'),
    [services],
  );

  if (isLoading) return <p className="text-sm text-subtle-ui py-4">{t('services.loading')}</p>;
  if (error != null) return <p className="text-sm text-rose-500 py-4">{t('services.loadError', { error: String(error) })}</p>;

  return (
    <>
      {downSvcs.length > 0 && (
        <div className="flex items-start gap-2 px-3 py-2.5 mb-3 rounded-lg bg-amber-500/10 border border-amber-500/30 text-amber-700 dark:text-amber-300 text-xs">
          <Warning size={14} weight="fill" className="shrink-0 mt-0.5" />
          <span>
            {downSvcs.length} service{downSvcs.length !== 1 ? 's' : ''} not running:{' '}
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
          isRowSelectable={(r) => r.kind !== 'bare'}
          selectedIds={selectedIds}
          onSelectionChange={setSelectedIds}
          onRowClick={(r) => {
            if (r.kind === 'bare') return;
            window.location.hash = `/instance/${project}/${name}/services/${encodeURIComponent(r.name)}`;
          }}
          emptyMessage={t('services.empty')}
        />
      </div>

      <Modal open={errorMsg != null} title={t('error.title')} onClose={() => setErrorMsg(null)}>
        <p className="text-rose-600 dark:text-rose-400 whitespace-pre-wrap">{errorMsg}</p>
      </Modal>
    </>
  );
}
