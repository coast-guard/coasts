import { useTranslation } from 'react-i18next';

export default function McpStatusBadge({ status }: { readonly status: string }) {
  const { t } = useTranslation();
  const colors: Record<string, string> = {
    installed: 'bg-green-500/15 text-green-700 dark:text-green-300',
    proxied: 'bg-yellow-500/15 text-yellow-700 dark:text-yellow-300',
    'not-installed': 'bg-red-500/15 text-red-700 dark:text-red-300',
  };
  const labels: Record<string, string> = {
    installed: t('mcp.status.installed'),
    proxied: t('mcp.status.proxied'),
    'not-installed': t('mcp.status.notInstalled'),
  };
  const cls = colors[status] ?? 'bg-gray-500/15 text-gray-600 dark:text-gray-400';
  return (
    <span className={`inline-block px-2 py-0.5 text-[10px] font-semibold rounded-full ${cls}`}>
      {labels[status] ?? status}
    </span>
  );
}
