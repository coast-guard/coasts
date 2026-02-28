import { useTranslation } from 'react-i18next';

export default function McpTypeBadge({ proxy }: { readonly proxy: string | null }) {
  const { t } = useTranslation();
  const isHost = proxy === 'host';
  return (
    <span
      className={`inline-block px-2 py-0.5 text-[10px] font-semibold rounded-full ${
        isHost
          ? 'bg-yellow-500/15 text-yellow-700 dark:text-yellow-300'
          : 'bg-blue-500/15 text-blue-700 dark:text-blue-300'
      }`}
    >
      {isHost ? t('mcp.type.host') : t('mcp.type.internal')}
    </span>
  );
}
