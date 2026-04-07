import { useTranslation } from 'react-i18next';

export interface ToolbarAction {
  readonly label: string;
  readonly variant: 'outline' | 'danger';
  readonly onClick: () => void;
}

interface ToolbarProps {
  readonly actions: readonly ToolbarAction[];
  readonly selectedCount: number;
  readonly memorySummary?: string | undefined;
  readonly memoryHighlight?: string | undefined;
}

export default function Toolbar({ actions, selectedCount, memorySummary, memoryHighlight }: ToolbarProps) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-2 flex-wrap px-4 py-2 bg-[var(--surface-muted)] border-b border-[var(--border)]">
      {actions.map((action) => (
        <button
          key={action.label}
          onClick={action.onClick}
          disabled={selectedCount === 0}
          className={`btn disabled:opacity-40 disabled:cursor-not-allowed ${
            action.variant === 'danger'
              ? 'btn-danger'
              : 'btn-outline'
          }`}
        >
          {action.label}
        </button>
      ))}
      <span className="ml-auto text-xs text-subtle-ui">
        {selectedCount > 0
          ? t('toolbar.selected', { count: selectedCount })
          : <>
              {memorySummary ?? null}
              {memoryHighlight && <span className="text-main font-semibold">{memoryHighlight}</span>}
            </>}
      </span>
    </div>
  );
}
