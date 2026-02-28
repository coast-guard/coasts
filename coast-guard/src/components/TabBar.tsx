import { Link } from 'react-router';
import { Warning } from '@phosphor-icons/react';

export interface TabDef<T extends string> {
  readonly id: T;
  readonly label: string;
  readonly to: string;
  readonly warn?: boolean;
}

interface TabBarProps<T extends string> {
  readonly tabs: readonly TabDef<T>[];
  readonly active: T;
}

export default function TabBar<T extends string>({ tabs, active }: TabBarProps<T>) {
  return (
    <nav className="mb-4 glass-subpanel p-1.5 flex gap-1 overflow-x-auto" style={{ scrollbarWidth: 'none' }}>
      {tabs.map((tab) => (
        <Link
          key={tab.id}
          to={tab.to}
          className={`shrink-0 px-4 py-2 text-sm font-semibold rounded-md transition-colors inline-flex items-center gap-1.5 ${
            tab.id === active
              ? 'bg-blue-500/15 text-blue-700 dark:text-blue-300'
              : 'text-muted-ui hover:text-main hover:bg-white/20 dark:hover:bg-white/8'
          }`}
        >
          {tab.label}
          {tab.warn === true && <Warning size={14} weight="fill" className="text-amber-500" />}
        </Link>
      ))}
    </nav>
  );
}
