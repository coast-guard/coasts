import { useMemo, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router';
import type { ProjectName } from '../types/branded';
import type { SharedServiceInfo } from '../types/api';
import { useSharedServices, useSharedStopMutation, useSharedStartMutation, useSharedRestartMutation, useSharedRmMutation } from '../api/hooks';
import { ApiError } from '../api/client';
import { useProjectMemory } from '../hooks/useProjectMemory';
import { formatBytes } from '../lib/formatBytes';
import DataTable, { type Column } from './DataTable';
import Toolbar, { type ToolbarAction } from './Toolbar';
import StatusBadge from './StatusBadge';
import Modal from './Modal';

interface Props {
  readonly project: ProjectName;
}

export default function SharedServicesPanel({ project }: Props) {
  const { t, i18n } = useTranslation();
  const { data, isLoading } = useSharedServices(project as string);
  const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(new Set());
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const stopMut = useSharedStopMutation();
  const startMut = useSharedStartMutation();
  const restartMut = useSharedRestartMutation();
  const rmMut = useSharedRmMutation();

  const services = data?.services ?? [];

  const { memoryMap, totalMemory } = useProjectMemory(
    project as string,
    services,
    '/api/v1/host-service/stats/stream',
    'service',
  );

  const selectedNames = useMemo(
    () => services.filter((s) => selectedIds.has(s.name)).map((s) => s.name),
    [services, selectedIds],
  );

  const batchAction = useCallback(
    async (action: (vars: { project: string; service: string }) => Promise<unknown>) => {
      const errors: string[] = [];
      for (const svc of selectedNames) {
        try {
          await action({ project: project as string, service: svc });
        } catch (e) {
          errors.push(`${svc}: ${e instanceof ApiError ? e.body.error : String(e)}`);
        }
      }
      setSelectedIds(new Set());
      if (errors.length > 0) setErrorMsg(errors.join('\n'));
    },
    [selectedNames, project],
  );

  const toolbarActions: readonly ToolbarAction[] = useMemo(
    () => [
      { label: t('action.stop'), variant: 'outline' as const, onClick: () => void batchAction((v) => stopMut.mutateAsync(v)) },
      { label: t('action.start'), variant: 'outline' as const, onClick: () => void batchAction((v) => startMut.mutateAsync(v)) },
      { label: t('shared.restart'), variant: 'outline' as const, onClick: () => void batchAction((v) => restartMut.mutateAsync(v)) },
      { label: t('action.remove'), variant: 'danger' as const, onClick: () => void batchAction((v) => rmMut.mutateAsync(v)) },
    ],
    [batchAction, stopMut, startMut, restartMut, rmMut, t, i18n.language],
  );

  const columns: readonly Column<SharedServiceInfo>[] = useMemo(
    () => [
      {
        key: 'name',
        header: t('col.service'),
        headerClassName: 'w-[22%]',
        className: 'w-[22%]',
        render: (r) => (
          <Link
            to={`/project/${project}/host-services/${encodeURIComponent(r.name)}`}
            className="font-medium text-[var(--primary)] hover:underline"
          >
            {r.name}
          </Link>
        ),
      },
      {
        key: 'status',
        header: t('col.status'),
        headerClassName: 'w-[18%]',
        className: 'w-[18%]',
        render: (r) => {
          const badgeStatus = r.status === 'running' ? 'running' : 'stopped';
          const mem = memoryMap.get(r.name);
          return (
            <div className="flex items-center gap-2">
              <StatusBadge status={badgeStatus} />
              {mem != null && r.status === 'running' && (
                <span className="text-[11px] text-muted-ui">{formatBytes(mem.memoryUsed)}</span>
              )}
            </div>
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
            to={`/project/${project}/host-images/${encodeURIComponent(r.image ?? '')}`}
            className="font-mono text-xs text-[var(--primary)] hover:underline truncate max-w-[200px] inline-block"
            title={r.image ?? ''}
            onClick={(e) => e.stopPropagation()}
          >
            {r.image ?? '—'}
          </Link>
        ),
      },
      {
        key: 'ports',
        header: t('col.ports'),
        render: (r) => {
          if (r.ports == null || r.ports.length === 0) {
            return <span className="text-subtle-ui text-xs">—</span>;
          }
          return (
            <div className="text-xs font-mono leading-5 text-subtle-ui">
              {r.ports.split(',').map((p, i) => (
                <span key={i}>{p.trim()}{i < r.ports!.split(',').length - 1 && <br />}</span>
              ))}
            </div>
          );
        },
      },
    ],
    [memoryMap, project, t, i18n.language],
  );

  if (isLoading) return null;

  if (services.length === 0) {
    return (
      <section className="mb-8">
        <div className="glass-panel p-8 text-center text-sm text-subtle-ui">
          {t('shared.empty')}
        </div>
      </section>
    );
  }

  return (
    <section className="mb-8">
      <h3 className="text-xs font-semibold uppercase tracking-wider text-subtle-ui mt-2 mb-4">
        {t('shared.title')}
        <span className="ml-2 font-normal normal-case tracking-normal">— {t('shared.subtitle')}</span>
      </h3>
      <div className="glass-panel overflow-hidden">
        <Toolbar
          actions={toolbarActions}
          selectedCount={selectedNames.length}
          memorySummary={totalMemory > 0 ? t('toolbar.memory', { memory: formatBytes(totalMemory) }) : undefined}
        />
        <DataTable
          columns={columns}
          data={services as SharedServiceInfo[]}
          getRowId={(r) => r.name}
          selectable
          selectedIds={selectedIds}
          onSelectionChange={setSelectedIds}
          onRowClick={(r) => {
            window.location.hash = `/project/${project}/host-services/${encodeURIComponent(r.name)}`;
          }}
          emptyMessage={t('shared.empty')}
        />
      </div>

      <Modal open={errorMsg != null} title={t('error.title')} onClose={() => setErrorMsg(null)}>
        <p className="text-rose-600 dark:text-rose-400 whitespace-pre-wrap">{errorMsg}</p>
      </Modal>
    </section>
  );
}
