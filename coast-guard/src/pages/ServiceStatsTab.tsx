import { useState, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import type { ProjectName, InstanceName } from '../types/branded';
import type { ContainerStats } from '../types/api';
import StatsChart, { type StatsPoint as ChartPoint } from '../components/StatsChart';

interface Props {
  readonly project: ProjectName;
  readonly name: InstanceName;
  readonly service: string;
}


interface FullStatsPoint {
  readonly time: Date;
  readonly cpuPercent: number;
  readonly memoryUsed: number;
  readonly memoryLimit: number;
  readonly memoryPercent: number;
  readonly diskRead: number;
  readonly diskWrite: number;
  readonly networkRx: number;
  readonly networkTx: number;
  readonly pids: number;
}

const MAX_POINTS = 300;

function parseContainerStats(raw: ContainerStats): FullStatsPoint {
  return {
    time: new Date(raw.timestamp),
    cpuPercent: raw.cpu_percent,
    memoryUsed: raw.memory_used_bytes,
    memoryLimit: raw.memory_limit_bytes,
    memoryPercent: raw.memory_percent,
    diskRead: raw.disk_read_bytes,
    diskWrite: raw.disk_write_bytes,
    networkRx: raw.network_rx_bytes,
    networkTx: raw.network_tx_bytes,
    pids: raw.pids,
  };
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)}GB`;
}

export default function ServiceStatsTab({ project, name, service }: Props) {
  const { t } = useTranslation();
  const [points, setPoints] = useState<FullStatsPoint[]>([]);
  const [status, setStatus] = useState<'connecting' | 'streaming' | 'closed' | 'error'>('connecting');
  const [latest, setLatest] = useState<FullStatsPoint | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const newestTimeRef = useRef<number>(0);

  // Reset state when switching services
  useEffect(() => {
    setPoints([]);
    setLatest(null);
    setStatus('connecting');
    newestTimeRef.current = 0;
  }, [project, name, service]);

  // Fetch history on mount
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const res = await fetch(`/api/v1/service/stats/history?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}&service=${encodeURIComponent(service)}`);
        if (!res.ok || cancelled) return;
        const history = (await res.json()) as ContainerStats[];
        const historicalPoints = history.map(parseContainerStats);
        if (!cancelled && historicalPoints.length > 0) {
          setPoints(historicalPoints.slice(-MAX_POINTS));
          setLatest(historicalPoints[historicalPoints.length - 1]!);
          newestTimeRef.current = historicalPoints[historicalPoints.length - 1]!.time.getTime();
        }
      } catch { /* ignore */ }
    })();
    return () => { cancelled = true; };
  }, [project, name, service]);

  // Live WebSocket stream
  useEffect(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${protocol}//${window.location.host}/api/v1/service/stats/stream?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}&service=${encodeURIComponent(service)}`;

    const ws = new WebSocket(url);
    wsRef.current = ws;

    ws.addEventListener('open', () => setStatus('streaming'));

    ws.addEventListener('message', (event: MessageEvent<string>) => {
      try {
        const raw = JSON.parse(event.data) as ContainerStats;
        if ('error' in raw) return;
        const point = parseContainerStats(raw);
        if (point.time.getTime() <= newestTimeRef.current) return;
        newestTimeRef.current = point.time.getTime();
        setLatest(point);
        setPoints((prev) => {
          const next = [...prev, point];
          return next.length > MAX_POINTS ? next.slice(-MAX_POINTS) : next;
        });
      } catch { /* ignore parse errors */ }
    });

    ws.addEventListener('close', () => setStatus('closed'));
    ws.addEventListener('error', () => setStatus('error'));

    return () => ws.close();
  }, [project, name, service]);

  const cpuData: ChartPoint[] = points.map(p => ({ time: p.time, value: p.cpuPercent }));
  const memData: ChartPoint[] = points.map(p => ({ time: p.time, value: p.memoryUsed }));
  const diskData: ChartPoint[] = points.map(p => ({ time: p.time, value: p.diskRead, value2: p.diskWrite }));
  const netData: ChartPoint[] = points.map(p => ({ time: p.time, value: p.networkRx, value2: p.networkTx }));

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-3 text-subtle-ui text-xs">
        <span className={`h-2 w-2 rounded-full ${
          status === 'streaming' ? 'bg-emerald-500 animate-pulse'
            : status === 'connecting' ? 'bg-amber-500 animate-pulse'
              : status === 'error' ? 'bg-rose-500' : 'bg-slate-400'
        }`} />
        <span>
          {status === 'connecting' && t('stats.connecting')}
          {status === 'streaming' && t('stats.streaming')}
          {status === 'closed' && t('stats.closed')}
          {status === 'error' && t('stats.error')}
        </span>
        {latest != null && (
          <>
            <span className="ml-auto">{t('stats.pids')}: <strong className="text-main">{latest.pids}</strong></span>
            <span>{t('stats.memory')}: <strong className="text-main">{formatBytes(latest.memoryUsed)} / {formatBytes(latest.memoryLimit)}</strong></span>
          </>
        )}
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div className="glass-panel p-4 flex flex-col gap-2 h-64">
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-semibold text-subtle-ui uppercase tracking-wider">{t('stats.cpuUsage')}</span>
            <span className="text-sm font-semibold text-main">{latest ? `${latest.cpuPercent.toFixed(1)}%` : '--'}</span>
          </div>
          <div className="flex-1 min-h-0">
            <StatsChart data={cpuData} color="#3b82f6" label="CPU" formatY={(v) => `${v.toFixed(0)}%`} />
          </div>
        </div>

        <div className="glass-panel p-4 flex flex-col gap-2 h-64">
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-semibold text-subtle-ui uppercase tracking-wider">{t('stats.memoryUsage')}</span>
            <span className="text-sm font-semibold text-main">{latest ? `${formatBytes(latest.memoryUsed)} / ${formatBytes(latest.memoryLimit)}` : '--'}</span>
          </div>
          <div className="flex-1 min-h-0">
            <StatsChart data={memData} color="#8b5cf6" label={t('stats.memUsed')} formatY={formatBytes} />
          </div>
        </div>

        <div className="glass-panel p-4 flex flex-col gap-2 h-64">
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-semibold text-subtle-ui uppercase tracking-wider">{t('stats.diskIO')}</span>
            <span className="text-sm font-semibold text-main">{latest ? `${formatBytes(latest.diskRead)} / ${formatBytes(latest.diskWrite)}` : '--'}</span>
          </div>
          <div className="flex-1 min-h-0">
            <StatsChart data={diskData} color="#06b6d4" label={t('stats.diskRead')} color2="#f97316" label2={t('stats.diskWrite')} formatY={formatBytes} />
          </div>
        </div>

        <div className="glass-panel p-4 flex flex-col gap-2 h-64">
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-semibold text-subtle-ui uppercase tracking-wider">{t('stats.networkIO')}</span>
            <span className="text-sm font-semibold text-main">{latest ? `${formatBytes(latest.networkRx)} / ${formatBytes(latest.networkTx)}` : '--'}</span>
          </div>
          <div className="flex-1 min-h-0">
            <StatsChart data={netData} color="#3b82f6" label={t('stats.netRx')} color2="#f97316" label2={t('stats.netTx')} formatY={formatBytes} />
          </div>
        </div>
      </div>
    </div>
  );
}
