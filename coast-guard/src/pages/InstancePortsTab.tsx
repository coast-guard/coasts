import { useMemo, useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { PencilSimple, Star, Globe } from '@phosphor-icons/react';
import type { ProjectName, InstanceName } from '../types/branded';
import type { PortMapping } from '../types/api';
import { usePorts } from '../api/hooks';
import { api } from '../api/endpoints';
import DataTable, { type Column } from '../components/DataTable';
import Modal from '../components/Modal';

interface Props {
  readonly project: ProjectName;
  readonly name: InstanceName;
  readonly checkedOut: boolean;
}

const DEFAULT_TEMPLATE = 'http://localhost:<port>';

function resolvePortUrl(template: string, port: number): string {
  return template.replace('<port>', String(port));
}

function validateTemplate(template: string): boolean {
  if (!template.includes('<port>')) return false;
  try {
    new URL(template.replace('<port>', '8080'));
    return true;
  } catch {
    return false;
  }
}

function settingsKey(project: string, service: string): string {
  return `port_url:${project}:${service}`;
}

function subdomainRoutingKey(project: string): string {
  return `subdomain_routing:${project}`;
}

export default function InstancePortsTab({ project, name, checkedOut }: Props) {
  const { t, i18n } = useTranslation();
  const qc = useQueryClient();
  const { data, isLoading, error, refetch } = usePorts(project, name);
  const [templates, setTemplates] = useState<Record<string, string>>({});
  const [editingService, setEditingService] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  const [subdomainRouting, setSubdomainRouting] = useState<boolean>(false);

  const ports = data?.ports ?? [];

  // Load per-service URL templates
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const result: Record<string, string> = {};
      for (const port of ports) {
        const val = await api.getSetting(settingsKey(project as string, port.logical_name));
        if (cancelled) return;
        if (val != null) result[port.logical_name] = val;
      }
      if (!cancelled) setTemplates(result);
    })();
    return () => { cancelled = true; };
  }, [ports.length, project]);

  // Load subdomain routing setting
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const val = await api.getSetting(subdomainRoutingKey(project as string));
      if (!cancelled) setSubdomainRouting(val === 'true');
    })();
    return () => { cancelled = true; };
  }, [project]);

  const getTemplate = useCallback(
    (service: string) => templates[service] ?? DEFAULT_TEMPLATE,
    [templates],
  );

  const applySubdomainRouting = useCallback(
    (url: string) => {
      if (!subdomainRouting) return url;
      return url.replace('localhost:', `${name as string}.localhost:`);
    },
    [subdomainRouting, name],
  );

  const openEdit = useCallback((service: string) => {
    setEditingService(service);
    setEditValue(templates[service] ?? DEFAULT_TEMPLATE);
  }, [templates]);

  const handleSave = useCallback(async () => {
    if (editingService == null) return;
    await api.setSetting(settingsKey(project as string, editingService), editValue);
    setTemplates((prev) => ({ ...prev, [editingService]: editValue }));
    setEditingService(null);
  }, [editingService, editValue, project]);

  const togglePrimary = useCallback(
    async (service: string, currentlyPrimary: boolean) => {
      if (currentlyPrimary) {
        await api.unsetPrimaryPort(name, project);
      } else {
        await api.setPrimaryPort(name, project, service);
      }
      void refetch();
    },
    [name, project, refetch],
  );

  const toggleSubdomainRouting = useCallback(async () => {
    const newValue = !subdomainRouting;
    await api.setSetting(subdomainRoutingKey(project as string), String(newValue));
    setSubdomainRouting(newValue);
    void qc.invalidateQueries({ queryKey: ['instances'] });
  }, [subdomainRouting, project, qc]);

  const isValid = validateTemplate(editValue);
  const editingPort = editingService != null
    ? ports.find((p) => p.logical_name === editingService)
    : null;

  const columns: readonly Column<PortMapping>[] = useMemo(
    () => [
      {
        key: 'service',
        header: t('col.service'),
        render: (r) => (
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={(e) => { e.stopPropagation(); void togglePrimary(r.logical_name, r.is_primary); }}
              className={`inline-flex items-center justify-center h-5 w-5 rounded transition-colors ${
                r.is_primary
                  ? 'text-yellow-400'
                  : 'text-subtle-ui hover:text-yellow-400 hover:bg-white/20 dark:hover:bg-white/10'
              }`}
              title={r.is_primary ? t('ports.unsetPrimary') : t('ports.setPrimary')}
            >
              <Star size={12} weight={r.is_primary ? 'fill' : 'regular'} />
            </button>
            <span className="font-medium">{r.logical_name}</span>
            <button
              type="button"
              onClick={(e) => { e.stopPropagation(); openEdit(r.logical_name); }}
              className="inline-flex items-center justify-center h-5 w-5 rounded text-subtle-ui hover:text-main hover:bg-white/20 dark:hover:bg-white/10 transition-colors"
              title={t('ports.editTemplate')}
            >
              <PencilSimple size={12} />
            </button>
          </div>
        ),
      },
      {
        key: 'canonical',
        header: t('col.canonical'),
        render: (r) => {
          const tmpl = getTemplate(r.logical_name);
          const url = resolvePortUrl(tmpl, r.canonical_port);
          if (!checkedOut) {
            return <span className="font-mono text-xs text-subtle-ui">{url}</span>;
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
          const tmpl = getTemplate(r.logical_name);
          const url = applySubdomainRouting(resolvePortUrl(tmpl, r.dynamic_port));
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
    [t, i18n.language, getTemplate, applySubdomainRouting, openEdit, checkedOut, togglePrimary],
  );

  if (isLoading) return <p className="text-sm text-subtle-ui py-4">{t('ports.loading')}</p>;
  if (error != null) return <p className="text-sm text-rose-500 py-4">{t('ports.loadError', { error: String(error) })}</p>;

  return (
    <>
      {/* Subdomain routing toggle */}
      <div className="flex items-center gap-3 rounded-lg border border-[var(--border)] px-4 py-2.5 mb-3">
        <Globe size={16} className={subdomainRouting ? 'text-[var(--primary)] shrink-0' : 'text-subtle-ui shrink-0'} />
        <p className="text-xs text-subtle-ui flex-1">
          {subdomainRouting
            ? <>Subdomain routing enabled — dynamic ports use <code className="text-[var(--primary)]">{name as string}.localhost</code></>
            : <>Use <code className="text-[var(--primary)]">{name as string}.localhost</code> subdomains for dynamic ports to avoid cookie collisions.</>
          }
        </p>
        <button
          type="button"
          className="btn btn-outline !px-2 !py-0.5 !text-[11px] shrink-0"
          onClick={() => void toggleSubdomainRouting()}
        >
          {subdomainRouting ? 'Disable' : 'Enable'}
        </button>
      </div>

      <div className="glass-panel overflow-hidden">
        <DataTable
          columns={columns}
          data={ports}
          getRowId={(r) => r.logical_name}
          emptyMessage={t('ports.empty')}
        />
      </div>

      <Modal
        open={editingService != null}
        title={t('ports.templateTitle', { service: editingService ?? '' })}
        onClose={() => setEditingService(null)}
        actions={
          <>
            <button
              type="button"
              className="btn btn-outline"
              onClick={() => setEditingService(null)}
            >
              {t('action.cancel')}
            </button>
            <button
              type="button"
              className="btn btn-primary"
              disabled={!isValid}
              onClick={() => void handleSave()}
            >
              {t('ports.save')}
            </button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <div>
            <label className="block text-xs font-semibold text-subtle-ui mb-1 uppercase tracking-wider">
              URL Template
            </label>
            <input
              type="text"
              value={editValue}
              onChange={(e) => setEditValue(e.target.value)}
              placeholder={t('ports.templatePlaceholder')}
              className="w-full h-9 px-3 text-sm font-mono rounded-md border border-[var(--border)] bg-transparent text-main outline-none focus:border-[var(--primary)]"
            />
          </div>

          {editingPort != null && isValid && (
            <div>
              <span className="block text-xs font-semibold text-subtle-ui mb-1 uppercase tracking-wider">
                {t('ports.templatePreview')}
              </span>
              <span className="text-xs font-mono text-[var(--primary)] break-all">
                {resolvePortUrl(editValue, editingPort.canonical_port)}
              </span>
            </div>
          )}

          {editValue.length > 0 && !isValid && (
            <p className="text-xs text-rose-500">{t('ports.templateInvalid')}</p>
          )}
        </div>
      </Modal>
    </>
  );
}
