import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import {
    ArrowDown,
    MagnifyingGlass,
    Asterisk,
    CornersOut,
    CornersIn,
} from '@phosphor-icons/react';
import { ssgLogsWsUrl } from '../api/ssgWsUrls';
import { parseLine, renderInstanceLogLine } from '../components/InstanceLogLine';

interface Props {
    readonly project: string;
    readonly service: string;
}

/**
 * SSG flavor of {@link ServiceLogsTab}: same status pill, search,
 * regex toggle, fullscreen, scroll-to-bottom FAB. Streams logs
 * from `docker compose logs --follow <service>` exec'd inside the
 * outer DinD via the existing
 * `/api/v1/ssg/logs/stream?project=<p>&service=<n>` endpoint.
 *
 * No clear-logs button: the SSG inner-compose stack doesn't expose
 * a per-service log-truncation API the way the per-instance
 * `coast logs --clear` path does, and `docker logs` drains stdout
 * progressively rather than from a host-side ring buffer.
 */
export default function SsgServiceLogsTab({ project, service }: Props) {
    const { t } = useTranslation();
    const [lines, setLines] = useState<string[]>([]);
    const [status, setStatus] = useState<
        'connecting' | 'streaming' | 'closed' | 'error'
    >('connecting');
    const [isAtBottom, setIsAtBottom] = useState(true);
    const [searchText, setSearchText] = useState('');
    const [isRegex, setIsRegex] = useState(false);
    const [fullscreen, setFullscreen] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);
    const autoScrollRef = useRef(true);

    const toggleFullscreen = useCallback(() => setFullscreen((prev) => !prev), []);

    useEffect(() => {
        if (!fullscreen) return;
        function onKey(e: KeyboardEvent) {
            if (e.key === 'Escape') setFullscreen(false);
        }
        document.addEventListener('keydown', onKey);
        return () => document.removeEventListener('keydown', onKey);
    }, [fullscreen]);

    const scrollToBottom = useCallback(() => {
        if (containerRef.current != null) {
            containerRef.current.scrollTop = containerRef.current.scrollHeight;
        }
    }, []);

    const handleScroll = useCallback(() => {
        const el = containerRef.current;
        if (el == null) return;
        const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        setIsAtBottom(atBottom);
        autoScrollRef.current = atBottom;
    }, []);

    useEffect(() => {
        // Reset on service change (the user navigated to a sibling
        // service detail page) so stale lines from the previous
        // service don't leak.
        setLines([]);
        setStatus('connecting');

        const ws = new WebSocket(ssgLogsWsUrl(project, service));
        ws.addEventListener('open', () => {
            setStatus('streaming');
            requestAnimationFrame(scrollToBottom);
        });
        ws.addEventListener('message', (event: MessageEvent<string>) => {
            setLines((prev) => [...prev, event.data]);
            if (autoScrollRef.current) requestAnimationFrame(scrollToBottom);
        });
        ws.addEventListener('close', () => setStatus('closed'));
        ws.addEventListener('error', () => setStatus('error'));
        return () => ws.close();
    }, [project, service, scrollToBottom]);

    const allLines = useMemo(
        () => lines.join('').split('\n').filter((l) => l.length > 0),
        [lines],
    );

    const searchRegex = useMemo((): RegExp | undefined => {
        if (searchText.length === 0) return undefined;
        try {
            return isRegex
                ? new RegExp(searchText, 'gi')
                : new RegExp(searchText.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi');
        } catch {
            return undefined;
        }
    }, [searchText, isRegex]);

    const parsedLines = useMemo(
        () => allLines.map((raw) => parseLine(raw)),
        [allLines],
    );

    const filtered = useMemo(() => {
        if (searchRegex == null) return parsedLines;
        return parsedLines.filter((p) => searchRegex.test(p.text));
    }, [parsedLines, searchRegex]);

    return (
        <div
            className={
                fullscreen
                    ? 'fixed inset-0 z-[200] flex flex-col gap-2 p-4 bg-[var(--surface-solid)] backdrop-blur-2xl'
                    : 'relative flex flex-col gap-2'
            }
        >
            <div className="glass-subpanel flex items-center gap-2 px-3 py-2 flex-wrap shrink-0">
                <span
                    className={`h-2 w-2 rounded-full shrink-0 ${
                        status === 'streaming'
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

                <span className="text-xs font-mono text-main font-semibold">{service}</span>

                <div className="h-4 w-px bg-[var(--border)] mx-1" />

                <div className="flex items-center gap-1 flex-1 min-w-[200px] max-w-[400px]">
                    <div className="flex-1 flex items-center gap-1.5 h-7 px-2 rounded-md border border-[var(--border)] bg-transparent">
                        <MagnifyingGlass size={14} className="text-subtle-ui shrink-0" />
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
                        className={`h-7 px-2 text-[10px] font-semibold rounded-md border transition-colors ${
                            isRegex
                                ? 'border-[var(--primary)] text-[var(--primary)] bg-[var(--primary)]/10'
                                : 'border-[var(--border)] text-subtle-ui hover:text-main'
                        }`}
                        title={t('logs.regexMode')}
                    >
                        <Asterisk size={14} />
                    </button>
                </div>

                <div className="ml-auto flex items-center gap-2">
                    <span className="text-xs text-subtle-ui">
                        {filtered.length !== allLines.length
                            ? `${filtered.length} / ${allLines.length}`
                            : `${allLines.length}`}{' '}
                        {t('logs.lines')}
                    </span>
                    <button
                        type="button"
                        onClick={toggleFullscreen}
                        className="h-8 w-8 inline-flex items-center justify-center rounded-lg text-subtle-ui hover:text-main hover:bg-white/25 dark:hover:bg-white/10 transition-colors shrink-0"
                        title={fullscreen ? t('logs.exitFullscreen') : t('logs.fullscreen')}
                    >
                        {fullscreen ? <CornersIn size={18} /> : <CornersOut size={18} />}
                    </button>
                </div>
            </div>

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
                        {allLines.length === 0 ? t('logs.empty') : t('logs.noMatch')}
                    </span>
                ) : (
                    filtered.map((p, i) => renderInstanceLogLine(p, i, searchRegex))
                )}
            </div>

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
