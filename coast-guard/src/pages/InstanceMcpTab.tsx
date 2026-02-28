import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { useMcpServers, useMcpLocations } from '../api/hooks';
import Section from '../components/Section';
import McpServerRow from '../components/McpServerRow';
import McpToolsPanel from '../components/McpToolsPanel';

interface Props {
  readonly project: string;
  readonly name: string;
}

export default function InstanceMcpTab({ project, name }: Props) {
  const { t } = useTranslation();
  const { data: serversData, isLoading: serversLoading, error: serversError } = useMcpServers(project, name);
  const { data: locationsData } = useMcpLocations(project, name);
  const [selectedServer, setSelectedServer] = useState<string | null>(null);

  if (serversLoading) {
    return <p className="text-sm text-subtle-ui">{t('mcp.loading')}</p>;
  }
  if (serversError) {
    return <p className="text-sm text-red-400">{t('mcp.error', { error: (serversError as Error).message })}</p>;
  }

  const servers = serversData?.servers ?? [];
  const locations = locationsData?.locations ?? [];

  return (
    <div>
      {/* Servers */}
      <Section title={t('mcp.title')}>
        {servers.length === 0 ? (
          <p className="text-xs text-subtle-ui">{t('mcp.empty')}</p>
        ) : (
          <div className="max-h-80 overflow-auto">
            <table className="w-full text-left">
              <thead>
                <tr className="border-b border-[var(--border)]">
                  <th className="py-1.5 px-2 text-[10px] font-semibold text-subtle-ui uppercase">{t('mcp.col.name')}</th>
                  <th className="py-1.5 px-2 text-[10px] font-semibold text-subtle-ui uppercase">{t('mcp.col.type')}</th>
                  <th className="py-1.5 px-2 text-[10px] font-semibold text-subtle-ui uppercase">{t('mcp.col.command')}</th>
                  <th className="py-1.5 px-2 text-[10px] font-semibold text-subtle-ui uppercase">{t('mcp.col.status')}</th>
                </tr>
              </thead>
              <tbody>
                {servers.map((srv) => (
                  <McpServerRow
                    key={srv.name}
                    server={srv}
                    selected={selectedServer === srv.name}
                    onSelect={() => setSelectedServer(selectedServer === srv.name ? null : srv.name)}
                  />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Section>

      {/* Tools */}
      <Section title={t('mcp.tools.title')}>
        {selectedServer ? (
          <McpToolsPanel project={project} name={name} server={selectedServer} />
        ) : (
          <p className="text-xs text-subtle-ui">{t('mcp.tools.select')}</p>
        )}
      </Section>

      {/* Locations */}
      <Section title={t('mcp.locations.title')}>
        {locations.length === 0 ? (
          <p className="text-xs text-subtle-ui">{t('mcp.locations.empty')}</p>
        ) : (
          <table className="w-full text-left">
            <thead>
              <tr className="border-b border-[var(--border)]">
                <th className="py-1.5 px-2 text-[10px] font-semibold text-subtle-ui uppercase">{t('mcp.locations.col.client')}</th>
                <th className="py-1.5 px-2 text-[10px] font-semibold text-subtle-ui uppercase">{t('mcp.locations.col.format')}</th>
                <th className="py-1.5 px-2 text-[10px] font-semibold text-subtle-ui uppercase">{t('mcp.locations.col.path')}</th>
              </tr>
            </thead>
            <tbody>
              {locations.map((loc) => (
                <tr key={loc.client} className="border-b border-[var(--border)] last:border-0">
                  <td className="py-2 px-2 text-xs font-mono text-main">{loc.client}</td>
                  <td className="py-2 px-2 text-xs text-main">{loc.format}</td>
                  <td className="py-2 px-2 text-xs font-mono text-main break-all">{loc.config_path}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Section>
    </div>
  );
}
