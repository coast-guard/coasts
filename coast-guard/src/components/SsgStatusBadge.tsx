import { useTranslation } from 'react-i18next';

interface StatusStyle {
    readonly dot: string;
    readonly bg: string;
    readonly text: string;
    readonly label: string;
}

const RUNNING: StatusStyle = {
    dot: 'bg-emerald-500',
    bg: 'bg-emerald-500/12 border border-emerald-500/30',
    text: 'text-emerald-700 dark:text-emerald-300',
    label: 'status.running',
};
const STOPPED: StatusStyle = {
    dot: 'bg-rose-500',
    bg: 'bg-rose-500/12 border border-rose-500/30',
    text: 'text-rose-700 dark:text-rose-300',
    label: 'status.stopped',
};
const STARTING: StatusStyle = {
    dot: 'bg-teal-500 animate-pulse',
    bg: 'bg-teal-500/12 border border-teal-500/30',
    text: 'text-teal-700 dark:text-teal-300',
    label: 'status.starting',
};
const STOPPING: StatusStyle = {
    dot: 'bg-pink-500 animate-pulse',
    bg: 'bg-pink-500/12 border border-pink-500/30',
    text: 'text-pink-700 dark:text-pink-300',
    label: 'status.stopping',
};
const ABSENT: StatusStyle = {
    dot: 'bg-slate-400',
    bg: 'bg-slate-500/12 border border-[var(--border)]',
    text: 'text-subtle-ui',
    label: 'ssg.statusAbsent',
};
const UNKNOWN: StatusStyle = {
    dot: 'bg-amber-500',
    bg: 'bg-amber-500/12 border border-amber-500/30',
    text: 'text-amber-700 dark:text-amber-300',
    label: 'status.unknown',
};

function styleFor(status: string | null | undefined): StatusStyle {
    if (status == null || status === 'absent') return ABSENT;
    switch (status) {
        case 'running':
            return RUNNING;
        case 'stopped':
            return STOPPED;
        case 'starting':
            return STARTING;
        case 'stopping':
            return STOPPING;
        default:
            return UNKNOWN;
    }
}

interface SsgStatusBadgeProps {
    readonly status: string | null | undefined;
}

/**
 * SSG-flavored counterpart to {@link StatusBadge}. Visual parity
 * (rounded-full pill + colored dot + status label) but accepts the
 * SSG state's free-form `Option<String>` status field instead of
 * the typed `InstanceStatus` enum.
 */
export default function SsgStatusBadge({ status }: SsgStatusBadgeProps) {
    const { t } = useTranslation();
    const s = styleFor(status);
    // Fall back to the raw status string when the localized label
    // is missing — better to surface "checked_out" verbatim than
    // a translation key.
    const label = t(s.label, { defaultValue: status ?? '' });
    return (
        <span
            className={`inline-flex items-center gap-1.5 px-2.5 py-0.5 text-xs font-medium rounded-full ${s.bg} ${s.text}`}
        >
            <span className={`h-1.5 w-1.5 rounded-full ${s.dot}`} />
            {label}
        </span>
    );
}
