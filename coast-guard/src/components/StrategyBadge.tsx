import { useTranslation } from 'react-i18next';

export default function StrategyBadge({ strategy }: { readonly strategy: string }) {
  const { t } = useTranslation();
  const styles: Record<string, string> = {
    isolated: 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/20',
    shared: 'bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20',
  };
  const labels: Record<string, string> = {
    isolated: t('volumes.strategyIsolated'),
    shared: t('volumes.strategyShared'),
  };
  return (
    <span className={`inline-block px-2 py-0.5 rounded-full text-[10px] font-semibold border ${styles[strategy] ?? 'bg-slate-500/10 text-subtle-ui border-[var(--border)]'}`}>
      {labels[strategy] ?? strategy}
    </span>
  );
}
