import { useTranslation } from 'react-i18next';
import type { InstanceStatus } from '../types/api';

const STATUS_STYLES: Readonly<Record<InstanceStatus, { readonly dot: string; readonly bg: string; readonly text: string; readonly label: string }>> = {
  provisioning: { dot: 'bg-violet-500 animate-pulse', bg: 'bg-violet-500/12 border border-violet-500/30', text: 'text-violet-700 dark:text-violet-300', label: 'status.provisioning' },
  assigning: { dot: 'bg-fuchsia-500 animate-pulse', bg: 'bg-fuchsia-500/12 border border-fuchsia-500/30', text: 'text-fuchsia-700 dark:text-fuchsia-300', label: 'status.assigning' },
  unassigning: { dot: 'bg-orange-500 animate-pulse', bg: 'bg-orange-500/12 border border-orange-500/30', text: 'text-orange-700 dark:text-orange-300', label: 'status.unassigning' },
  starting: { dot: 'bg-teal-500 animate-pulse', bg: 'bg-teal-500/12 border border-teal-500/30', text: 'text-teal-700 dark:text-teal-300', label: 'status.starting' },
  stopping: { dot: 'bg-pink-500 animate-pulse', bg: 'bg-pink-500/12 border border-pink-500/30', text: 'text-pink-700 dark:text-pink-300', label: 'status.stopping' },
  running: { dot: 'bg-emerald-500', bg: 'bg-emerald-500/12 border border-emerald-500/30', text: 'text-emerald-700 dark:text-emerald-300', label: 'status.running' },
  stopped: { dot: 'bg-rose-500', bg: 'bg-rose-500/12 border border-rose-500/30', text: 'text-rose-700 dark:text-rose-300', label: 'status.stopped' },
  checked_out: { dot: 'bg-blue-500', bg: 'bg-blue-500/12 border border-blue-500/30', text: 'text-blue-700 dark:text-blue-300', label: 'status.checkedOut' },
  idle: { dot: 'bg-amber-500', bg: 'bg-amber-500/12 border border-amber-500/30', text: 'text-amber-700 dark:text-amber-300', label: 'status.idle' },
};

export default function StatusBadge({ status }: { readonly status: InstanceStatus }) {
  const { t } = useTranslation();
  const s = STATUS_STYLES[status];
  return (
    <span className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 text-xs font-medium rounded-full ${s.bg} ${s.text}`}>
      <span className={`h-1.5 w-1.5 rounded-full ${s.dot}`} />
      {t(s.label)}
    </span>
  );
}
