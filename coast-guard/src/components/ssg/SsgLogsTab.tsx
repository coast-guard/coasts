import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import {
    ArrowDown,
    Trash,
    MagnifyingGlass,
    Asterisk,
    CornersOut,
    CornersIn,
    Copy,
} from '@phosphor-icons/react';
import { api } from '../../api/endpoints';
import { useSsgState } from '../../api/hooks';
import {
    ssgLogsWsUrl,
    SSG_LOGS_SERVICE_ALL,
    SSG_LOGS_SERVICE_OUTER,
} from '../../api/ssgWsUrls';
import { parseLine, renderInstanceLogLine } from '../InstanceLogLine';

interface SsgLogsTabProps {
    readonly project: string;
}

/**
 * SSG → Logs tab. Visual + behavioural parity with
 * {@link InstanceLogsTab}: streamed text panel with parsed
 * service tags, search/regex, copy/clear, fullscreen, status
 * indicator, scroll-to-bottom FAB.
 *
 * Three sources, picked by the service dropdown:
 *
 * - **All services** (default): `docker compose logs --follow`
 *   inside the outer DinD streams every inner service in one
 *   feed. Compose prefixes lines with `<service>-1  | `, which
 *   the shared `parseLine` helper turns into per-service color
 *   tags — same UX as the regular instance logs.
 * - **Outer DinD**: raw stdout/stderr of the outer DinD
 *   container itself. Useful when debugging the inner Docker
 *   daemon (containerd boot, image pulls).
 * - **Specific service**: `docker compose logs --follow <name>`
 *   inside the DinD. Lines still carry the compose prefix.
 *
 * Service filter is enforced on the daemon side (the WS routes
 * to a different command per filter) instead of client-side
 * because the SSG can host services with very chatty outputs
 * and we don't want to ship every byte just to discard most.
 */
export default function SsgLogsTab({ project }: SsgLogsTabProps) {
    const { t } = useTranslation();
    const { data: ssgState } = useSsgState(project);

    const [lines, setLines] = useState<string[]>([]);
    const [status, setStatus] = useState<
        'connecting' | 'streaming' | 'closed' | 'error'
    >('connecting');
    const [isAtBottom, setIsAtBottom] = useState(true);
    const [serviceFilter, setServiceFilter] = useState<string>(
        SSG_LOGS_SERVICE_ALL,
    );
    const [searchText, setSearchText] = useState('');
    const [isRegex, setIsRegex] = useState(false);
    const [fullscreen, setFullscreen] = useState(false);
    const [copied, setCopied] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);
    const wsRef = useRef<WebSocket | null>(null);
    const autoScrollRef = useRef(true);
    const hydratedRef = useRef(false);

    const toggleFullscreen = useCallback(
        () => setFullscreen((prev) => !prev),
        [],
    );

    useEffect(() => {
        if (!fullscreen) {
            return;
        }
        function onKey(e: KeyboardEvent) {
            if (e.key === 'Escape') {
                setFullscreen(false);
            }
        }
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [fullscreen]);

    // Hydrate persisted filter state on mount. Settings keys are
    // namespaced under `logs:<project>:ssg` to avoid colliding
    // with per-instance overrides on the same project.
    useEffect(() => {
        const prefix = `logs:${project}:ssg`;
        Promise.all([
            api.getSetting(`${prefix}:service`),
            api.getSetting(`${prefix}:search`),
            api.getSetting(`${prefix}:regex`),
        ])
            .then(([svc, search, regex]) => {
                if (svc != null && svc.length > 0) {
                    setServiceFilter(svc);
                }
                if (search != null) {
                    setSearchText(search);
                }
                if (regex != null) {
                    setIsRegex(regex === '1');
                }
                hydratedRef.current = true;
            })
            .catch(() => {
                hydratedRef.current = true;
            });
    }, [project]);

    // Persist filter changes (debounced).
    useEffect(() => {
        if (!hydratedRef.current) {
            return;
        }
        const prefix = `logs:${project}:ssg`;
        const timer = setTimeout(() => {
            void api.setSetting(`${prefix}:service`, serviceFilter);
            void api.setSetting(`${prefix}:search`, searchText);
            void api.setSetting(`${prefix}:regex`, isRegex ? '1' : '0');
        }, 300);
        return () => clearTimeout(timer);
    }, [serviceFilter, searchText, isRegex, project]);

    const scrollToBottom = useCallback(() => {
        if (containerRef.current != null) {
            containerRef.current.scrollTop = containerRef.current.scrollHeight;
        }
    }, []);

    const handleScroll = useCallback(() => {
        const el = containerRef.current;
        if (el == null) {
            return;
        }
        const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        setIsAtBottom(atBottom);
        autoScrollRef.current = atBottom;
    }, []);

    // Reconnect whenever the daemon-side service filter changes.
    // Different filters = different `docker compose logs` invocations,
    // so a fresh WS keeps the UX simple (no need to merge feeds).
    useEffect(() => {
        setStatus('connecting');
        setLines([]);
        const url = ssgLogsWsUrl(project, serviceFilter);
        const ws = new WebSocket(url);
        wsRef.current = ws;
        ws.addEventListener('open', () => {
            setStatus('streaming');
            requestAnimationFrame(scrollToBottom);
        });
        ws.addEventListener('message', (event: MessageEvent<string>) => {
            setLines((prev) => [...prev, event.data]);
            if (autoScrollRef.current) {
                requestAnimationFrame(scrollToBottom);
            }
        });
        ws.addEventListener('close', () => setStatus('closed'));
        ws.addEventListener('error', () => setStatus('error'));
        return () => ws.close();
    }, [project, serviceFilter, scrollToBottom]);

    useEffect(() => {
        if (lines.length > 0) {
            requestAnimationFrame(scrollToBottom);
        }
    }, [lines.length > 0, scrollToBottom]);

    const handleClear = useCallback(() => {
        // SSG logs are emitted by Docker — there's no persistent
        // log file to truncate the way `api.clearLogs` does for
        // instances. Just dropping the local buffer is consistent
        // with what the user sees.
        setLines([]);
    }, []);

    const allParsed = useMemo(
        () =>
            lines
                .join('')
                .split('\n')
                .filter((l) => l.length > 0)
                .map(parseLine),
        [lines],
    );

    // Compose strips the trailing `-1` from container names on
    // newer versions and keeps it on older ones. Normalise both
    // forms to the configured service name (postgres, redis, …)
    // so the dropdown options match what the user actually sees
    // tagged on each line.
    const ssgServices = useMemo(
        () => ssgState?.services.map((s) => s.name) ?? [],
        [ssgState?.services],
    );

    const searchRegex = useMemo((): RegExp | undefined => {
        if (searchText.length === 0) {
            return undefined;
        }
        try {
            return isRegex
                ? new RegExp(searchText, 'gi')
                : new RegExp(
                    searchText.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'),
                    'gi',
                );
        } catch {
            return undefined;
        }
    }, [searchText, isRegex]);

    const filtered = useMemo(() => {
        let result = allParsed;
        if (searchRegex != null) {
            result = result.filter((p) => searchRegex.test(p.raw));
        }
        return result;
    }, [allParsed, searchRegex]);

    const handleCopy = useCallback(async () => {
        const text = filtered.map((p) => p.raw).join('\n');
        try {
            await navigator.clipboard.writeText(text);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        } catch {
            /* clipboard write blocked; ignore */
        }
    }, [filtered]);

    return (
        <div
            className={
                fullscreen
                    ? 'fixed inset-0 z-[200] flex flex-col gap-2 p-4 bg-[var(--surface-solid)] backdrop-blur-2xl'
                    : 'relative flex flex-col gap-2'
            }
        >
            {/* Toolbar */}
            <div className="glass-subpanel flex items-center gap-2 px-3 py-2 flex-wrap shrink-0">
                {/* Status */}
                <span
                    className={`h-2 w-2 rounded-full shrink-0 ${status === 'streaming'
                        ? 'bg-emerald-500 animate-pulse'
                        : status === 'connecting'
                            ? 'bg-amber-500 animate-pulse'
                            : status === 'error'
                                ? 'bg-rose-500'
                                : 'bg-slate-400'
                        }`}
                />
                <span className="text-xs text-subtle-ui shrink-0">
                    {status === 'connecting' && t('logs.connecting')}
                    {status === 'streaming' && t('logs.streaming')}
                    {status === 'closed' && t('logs.closed')}
                    {status === 'error' && t('logs.error')}
                </span>

                <div className="h-4 w-px bg-[var(--border)] mx-1" />

                {/* Service / source filter */}
                <select
                    value={serviceFilter}
                    onChange={(e) => setServiceFilter(e.target.value)}
                    className="h-7 pl-3 pr-8 text-xs font-mono rounded-md bg-transparent border border-[var(--border)] text-main outline-none appearance-none bg-[length:12px_12px] bg-[position:right_8px_center] bg-no-repeat"
                    style={{
                        backgroundImage:
                            "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath d='M3 4.5L6 7.5L9 4.5' fill='none' stroke='currentColor' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E\")",
                    }}
                >
                    <option value={SSG_LOGS_SERVICE_ALL}>
                        {t('logs.allServices')}
                    </option>
                    <option value={SSG_LOGS_SERVICE_OUTER}>
                        {t('ssg.logs.outerDind')}
                    </option>
                    {ssgServices.map((name) => (
                        <option key={name} value={name}>
                            {name}
                        </option>
                    ))}
                </select>

                <div className="h-4 w-px bg-[var(--border)] mx-1" />

                {/* Search */}
                <div className="flex items-center gap-1 flex-1 min-w-[200px] max-w-[400px]">
                    <div className="flex-1 flex items-center gap-1.5 h-7 px-2 rounded-md border border-[var(--border)] bg-transparent">
                        <MagnifyingGlass
                            size={14}
                            className="text-subtle-ui shrink-0"
                        />
                        <input
                            type="text"
                            value={searchText}
                            onChange={(e) => setSearchText(e.target.value)}
                            placeholder={t('logs.searchPlaceholder')}
                            className="flex-1 bg-transparent text-xs text-main outline-none placeholder:text-subtle-ui"
                        />
                    </div>
                    <button
                        type="button"
                        onClick={() => setIsRegex((v) => !v)}
                        className={`h-7 px-2 text-[10px] font-semibold rounded-md border transition-colors ${isRegex
                            ? 'border-[var(--primary)] text-[var(--primary)] bg-[var(--primary)]/10'
                            : 'border-[var(--border)] text-subtle-ui hover:text-main'
                            }`}
                        title={t('logs.regexMode')}
                    >
                        <Asterisk size={14} />
                    </button>
                </div>

                {/* Right side */}
                <div className="ml-auto flex items-center gap-2">
                    <span className="text-xs text-subtle-ui">
                        {filtered.length !== allParsed.length
                            ? `${filtered.length} / ${allParsed.length}`
                            : `${allParsed.length}`}{' '}
                        {t('logs.lines')}
                    </span>
                    <button
                        type="button"
                        className="btn btn-outline !px-2 !py-1 !text-xs inline-flex items-center gap-1.5"
                        onClick={() => void handleCopy()}
                    >
                        <Copy size={14} />
                        {copied ? t('action.copied') : t('action.copy')}
                    </button>
                    <button
                        type="button"
                        className="btn btn-outline !px-2 !py-1 !text-xs inline-flex items-center gap-1.5"
                        onClick={handleClear}
                    >
                        <Trash size={14} />
                        {t('action.clear')}
                    </button>
                    <button
                        type="button"
                        onClick={toggleFullscreen}
                        className="h-8 w-8 inline-flex items-center justify-center rounded-lg text-subtle-ui hover:text-main hover:bg-white/25 dark:hover:bg-white/10 transition-colors shrink-0"
                        title={
                            fullscreen
                                ? t('logs.exitFullscreen')
                                : t('logs.fullscreen')
                        }
                    >
                        {fullscreen ? <CornersIn size={18} /> : <CornersOut size={18} />}
                    </button>
                </div>
            </div>

            {/* Log output */}
            <div
                ref={containerRef}
                onScroll={handleScroll}
                className={
                    fullscreen
                        ? 'glass-panel flex-1 min-h-0 overflow-auto p-4 text-xs font-mono'
                        : 'glass-panel h-[calc(100vh-420px)] min-h-[300px] overflow-auto p-4 text-xs font-mono'
                }
            >
                {filtered.length === 0 ? (
                    <span className="text-subtle-ui">
                        {allParsed.length === 0
                            ? t('logs.empty')
                            : t('logs.noMatch')}
                    </span>
                ) : (
                    filtered.map((line, i) =>
                        renderInstanceLogLine(line, i, searchRegex),
                    )
                )}
            </div>

            {/* Scroll to bottom FAB */}
            {!isAtBottom && (
                <button
                    type="button"
                    onClick={() => {
                        scrollToBottom();
                        autoScrollRef.current = true;
                        setIsAtBottom(true);
                    }}
                    className="absolute bottom-6 right-6 h-9 w-9 inline-flex items-center justify-center rounded-full bg-[var(--primary)] text-white shadow-lg hover:opacity-90 transition-opacity"
                    title={t('logs.scrollToBottom')}
                >
                    <ArrowDown size={18} weight="bold" />
                </button>
            )}
        </div>
    );
}
