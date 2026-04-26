import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { SsgStatsSample } from '../../types/api';
import { ssgStatsWsUrl } from '../../api/ssgWsUrls';

interface SsgStatsTabProps {
    readonly project: string;
}

function formatBytes(bytes: number): string {
    if (bytes >= 1_073_741_824) {
        return `${(bytes / 1_073_741_824).toFixed(2)} GB`;
    }
    if (bytes >= 1_048_576) {
        return `${(bytes / 1_048_576).toFixed(1)} MB`;
    }
    if (bytes >= 1024) {
        return `${(bytes / 1024).toFixed(1)} KB`;
    }
    return `${bytes} B`;
}

function memPct(used: number, limit: number): string {
    if (limit <= 0) {
        return '—';
    }
    return `${((used / limit) * 100).toFixed(1)}%`;
}

/**
 * Live `docker stats` for the SSG outer DinD container. Receives
 * one JSON `SsgStatsSample` frame per second over websocket and
 * renders the latest sample. (No history chart in v1; the parent
 * page is plenty of structure for a follow-up if needed.)
 */
export default function SsgStatsTab({ project }: SsgStatsTabProps) {
    const { t } = useTranslation();
    const [latest, setLatest] = useState<SsgStatsSample | null>(null);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        const ws = new WebSocket(ssgStatsWsUrl(project));
        ws.addEventListener('message', (event: MessageEvent<string>) => {
            try {
                const sample: SsgStatsSample = JSON.parse(event.data);
                setLatest(sample);
            } catch {
                // ignore non-JSON frames (e.g. error text)
            }
        });
        ws.addEventListener('error', () => {
            setError('Connection error');
        });
        ws.addEventListener('close', () => {
            // Keep last sample visible after close.
        });
        return () => {
            ws.close();
        };
    }, [project]);

    if (error != null) {
        return (
            <section className="glass-panel p-6 text-sm text-rose-600 dark:text-rose-400">
                {error}
            </section>
        );
    }

    if (latest == null) {
        return (
            <section className="glass-panel p-6 text-sm text-subtle-ui">
                Waiting for first stats sample…
            </section>
        );
    }

    return (
        <section className="glass-panel p-5 space-y-4">
            <h3 className="text-sm font-semibold text-main">
                {t('ssg.stats.title')}
            </h3>
            <div className="grid grid-cols-2 gap-y-3 gap-x-6 text-sm">
                <span className="text-subtle-ui">{t('ssg.stats.cpu')}</span>
                <span className="text-main font-mono">
                    {latest.cpu_pct.toFixed(2)}%
                </span>
                <span className="text-subtle-ui">{t('ssg.stats.mem')}</span>
                <span className="text-main font-mono">
                    {formatBytes(latest.mem_used_bytes)} /{' '}
                    {formatBytes(latest.mem_limit_bytes)} (
                    {memPct(latest.mem_used_bytes, latest.mem_limit_bytes)})
                </span>
                <span className="text-subtle-ui">{t('ssg.stats.netRx')}</span>
                <span className="text-main font-mono">
                    {formatBytes(latest.net_rx_bytes)}
                </span>
                <span className="text-subtle-ui">{t('ssg.stats.netTx')}</span>
                <span className="text-main font-mono">
                    {formatBytes(latest.net_tx_bytes)}
                </span>
                <span className="text-subtle-ui">{t('ssg.stats.blockRead')}</span>
                <span className="text-main font-mono">
                    {formatBytes(latest.block_read_bytes)}
                </span>
                <span className="text-subtle-ui">{t('ssg.stats.blockWrite')}</span>
                <span className="text-main font-mono">
                    {formatBytes(latest.block_write_bytes)}
                </span>
            </div>
            <p className="text-xs text-subtle-ui">
                {t('ssg.stats.note')}
            </p>
        </section>
    );
}
