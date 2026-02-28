import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useMcpTools } from '../api/hooks';

export default function McpToolsPanel({
  project,
  name,
  server,
}: {
  readonly project: string;
  readonly name: string;
  readonly server: string;
}) {
  const { t } = useTranslation();
  const [selectedTool, setSelectedTool] = useState<string | null>(null);
  const { data: toolsData, isLoading, error } = useMcpTools(project, name, server);
  const { data: toolInfoData } = useMcpTools(project, name, server, selectedTool ?? undefined);

  if (isLoading) {
    return <p className="text-xs text-subtle-ui py-2">{t('mcp.tools.loading')}</p>;
  }
  if (error) {
    return <p className="text-xs text-red-400 py-2">{(error as Error).message}</p>;
  }
  if (!toolsData || toolsData.tools.length === 0) {
    return <p className="text-xs text-subtle-ui py-2">{t('mcp.tools.empty')}</p>;
  }

  const info = toolInfoData?.tool_info;

  return (
    <div>
      <div className="max-h-60 overflow-auto">
        <table className="w-full text-left">
          <thead>
            <tr className="border-b border-[var(--border)]">
              <th className="py-1.5 px-2 text-[10px] font-semibold text-subtle-ui uppercase">{t('mcp.col.name')}</th>
              <th className="py-1.5 px-2 text-[10px] font-semibold text-subtle-ui uppercase">{t('mcp.col.description')}</th>
            </tr>
          </thead>
          <tbody>
            {toolsData.tools.map((tool) => (
              <tr
                key={tool.name}
                className={`border-b border-[var(--border)] last:border-0 cursor-pointer transition-colors ${
                  selectedTool === tool.name ? 'bg-blue-500/10' : 'hover:bg-white/5'
                }`}
                onClick={() => setSelectedTool(selectedTool === tool.name ? null : tool.name)}
              >
                <td className="py-2 px-2 text-xs font-mono text-main">{tool.name}</td>
                <td className="py-2 px-2 text-xs text-main">{tool.description}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {info && (
        <div className="mt-3 p-3 rounded-md bg-black/5 dark:bg-white/5">
          <div className="flex gap-3 mb-2">
            <span className="text-xs text-subtle-ui font-medium w-24 shrink-0">{t('mcp.col.name')}</span>
            <span className="text-xs font-mono text-main">{info.name}</span>
          </div>
          <div className="flex gap-3 mb-2">
            <span className="text-xs text-subtle-ui font-medium w-24 shrink-0">{t('mcp.col.description')}</span>
            <span className="text-xs text-main">{info.description}</span>
          </div>
          <div>
            <span className="text-xs text-subtle-ui font-medium">{t('mcp.tools.schema')}</span>
            <pre className="mt-1 text-[11px] font-mono text-main bg-black/5 dark:bg-white/5 p-2 rounded overflow-auto max-h-48">
              {JSON.stringify(info.input_schema, null, 2)}
            </pre>
          </div>
        </div>
      )}
    </div>
  );
}
