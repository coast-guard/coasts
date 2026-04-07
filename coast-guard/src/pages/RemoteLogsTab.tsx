import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowDown, MagnifyingGlass, Asterisk, CornersOut, CornersIn, Copy } from '@phosphor-icons/react';

interface Props {
  readonly remoteName: string;
}

export default function RemoteLogsTab({ remoteName }: Props) {
  const { t } = useTranslation();
  const [lines, setLines] = useState<string[]>([]);
  const [status, setStatus] = useState<'connecting' | 'streaming' | 'closed' | 'error'>('connecting');
  const [isAtBottom, setIsAtBottom] = useState(true);
  const [searchText, setSearchText] = useState('');
  const [isRegex, setIsRegex] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [copied, setCopied] = useState(false);
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

  useEffect(() => {
    setLines([]);
    setStatus('connecting');

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${protocol}//${window.location.host}/api/v1/remote/logs/stream?name=${encodeURIComponent(remoteName)}`;

    const ws = new WebSocket(url);

    ws.addEventListener('open', () => setStatus('streaming'));

    ws.addEventListener('message', (event: MessageEvent<string>) => {
      setLines((prev) => {
        const next = [...prev, event.data];
        return next.length > 10_000 ? next.slice(-10_000) : next;
      });
    });

    ws.addEventListener('close', () => setStatus('closed'));
    ws.addEventListener('error', () => setStatus('error'));

    return () => ws.close();
  }, [remoteName]);

  useEffect(() => {
    if (autoScrollRef.current && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [lines]);

  const handleScroll = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    setIsAtBottom(atBottom);
    autoScrollRef.current = atBottom;
  }, []);

  const scrollToBottom = useCallback(() => {
    containerRef.current?.scrollTo({ top: containerRef.current.scrollHeight, behavior: 'smooth' });
  }, []);

  const allLines = useMemo(() => {
    return lines.join('').split('\n').filter((l) => l.length > 0);
  }, [lines]);

  const searchRegex = useMemo(() => {
    if (!searchText) return null;
    try {
      return isRegex ? new RegExp(searchText, 'i') : new RegExp(searchText.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'i');
    } catch {
      return null;
    }
  }, [searchText, isRegex]);

  const filtered = useMemo(() => {
    if (!searchRegex) return allLines;
    return allLines.filter((l) => searchRegex.test(l));
  }, [allLines, searchRegex]);

  const handleCopy = useCallback(async () => {
    const text = filtered.join('\n');
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch { /* ignore */ }
  }, [filtered]);

  return (
    <div className={fullscreen
      ? 'fixed inset-0 z-[200] flex flex-col gap-2 p-4 bg-[var(--surface-solid)] backdrop-blur-2xl'
      : 'relative flex flex-col gap-2'
    }>
      {/* Toolbar */}
      <div className="glass-subpanel flex items-center gap-2 px-3 py-2 flex-wrap shrink-0">
        <span className={`h-2 w-2 rounded-full shrink-0 ${
          status === 'streaming' ? 'bg-emerald-500 animate-pulse'
            : status === 'connecting' ? 'bg-amber-500 animate-pulse'
              : status === 'error' ? 'bg-rose-500' : 'bg-slate-400'
        }`} />
        <span className="text-xs text-subtle-ui shrink-0">
          {status === 'connecting' && t('logs.connecting')}
          {status === 'streaming' && t('logs.streaming')}
          {status === 'closed' && t('logs.closed')}
          {status === 'error' && t('logs.error')}
        </span>

        <div className="h-4 w-px bg-[var(--border)] mx-1" />

        {/* Search */}
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

        {/* Right side */}
        <div className="ml-auto flex items-center gap-2">
          <span className="text-xs text-subtle-ui">
            {filtered.length !== allLines.length
              ? `${filtered.length} / ${allLines.length}`
              : `${allLines.length}`
            } {t('logs.lines')}
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
            onClick={toggleFullscreen}
            className="h-8 w-8 inline-flex items-center justify-center rounded-lg text-subtle-ui hover:text-main hover:bg-white/25 dark:hover:bg-white/10 transition-colors shrink-0"
            title={fullscreen ? t('logs.exitFullscreen') : t('logs.fullscreen')}
          >
            {fullscreen ? <CornersIn size={18} /> : <CornersOut size={18} />}
          </button>
        </div>
      </div>

      {/* Log output */}
      <div
        ref={containerRef}
        onScroll={handleScroll}
        className={fullscreen
          ? 'glass-panel flex-1 min-h-0 overflow-auto p-4 text-xs font-mono'
          : 'glass-panel h-[calc(100vh-420px)] min-h-[300px] overflow-auto p-4 text-xs font-mono'
        }
      >
        {filtered.length === 0 ? (
          <span className="text-subtle-ui">
            {allLines.length === 0 ? t('logs.empty') : t('logs.noMatch')}
          </span>
        ) : (
          filtered.map((line, i) => (
            <div key={i} className="whitespace-pre-wrap break-all leading-5 text-main">
              {searchRegex ? highlightMatches(line, searchRegex) : line}
            </div>
          ))
        )}
      </div>

      {/* Scroll to bottom FAB */}
      {!isAtBottom && (
        <button
          type="button"
          onClick={() => { scrollToBottom(); autoScrollRef.current = true; setIsAtBottom(true); }}
          className="absolute bottom-6 right-6 h-9 w-9 inline-flex items-center justify-center rounded-full bg-[var(--primary)] text-white shadow-lg hover:opacity-90 transition-opacity"
          title={t('logs.scrollToBottom')}
        >
          <ArrowDown size={18} weight="bold" />
        </button>
      )}
    </div>
  );
}

function highlightMatches(text: string, regex: RegExp): React.ReactNode {
  const parts: React.ReactNode[] = [];
  let lastIndex = 0;
  const globalRegex = new RegExp(regex.source, 'gi');
  let match: RegExpExecArray | null;
  let key = 0;

  while ((match = globalRegex.exec(text)) !== null) {
    if (match.index > lastIndex) {
      parts.push(text.slice(lastIndex, match.index));
    }
    parts.push(
      <mark key={key++} className="bg-yellow-300/40 dark:bg-yellow-500/30 text-inherit rounded-sm px-0.5">
        {match[0]}
      </mark>
    );
    lastIndex = globalRegex.lastIndex;
    if (match[0].length === 0) {
      globalRegex.lastIndex++;
    }
  }

  if (lastIndex < text.length) {
    parts.push(text.slice(lastIndex));
  }

  return parts.length > 0 ? <>{parts}</> : text;
}
