import { useEffect, useRef, useState, useMemo } from 'react';
import type { ContainerStats } from '../types/api';

export interface MemoryInfo {
  readonly memoryUsed: number;
  readonly memoryLimit: number;
}

const RUNNING_STATUSES = new Set(['running', 'checked_out', 'idle']);
const FLUSH_INTERVAL = 2000;

/**
 * Opens one lightweight WebSocket per running entity to track its latest
 * memory usage. Returns a per-entity map and the aggregate total.
 */
export function useProjectMemory(
  project: string,
  entities: readonly { name: string; status: string }[],
  streamPath: string,
  entityParam: string,
): { memoryMap: ReadonlyMap<string, MemoryInfo>; totalMemory: number } {
  const [memoryMap, setMemoryMap] = useState<Map<string, MemoryInfo>>(new Map());
  const latestRef = useRef<Map<string, MemoryInfo>>(new Map());
  const wsRef = useRef<Map<string, WebSocket>>(new Map());
  const needsImmediateFlush = useRef(true);

  const runningNames = useMemo(
    () =>
      entities
        .filter((e) => RUNNING_STATUSES.has(e.status))
        .map((e) => e.name)
        .sort()
        .join(','),
    [entities],
  );

  useEffect(() => {
    needsImmediateFlush.current = true;
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const desired = new Set(runningNames ? runningNames.split(',') : []);
    const current = wsRef.current;

    for (const [name, ws] of current) {
      if (!desired.has(name)) {
        if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
          ws.close();
        }
        current.delete(name);
        latestRef.current.delete(name);
      }
    }

    for (const name of desired) {
      if (current.has(name)) continue;

      const url = `${protocol}//${window.location.host}${streamPath}?project=${encodeURIComponent(project)}&${entityParam}=${encodeURIComponent(name)}`;
      const ws = new WebSocket(url);

      ws.addEventListener('message', (event: MessageEvent<string>) => {
        try {
          const raw = JSON.parse(event.data) as ContainerStats;
          if ('error' in raw) return;
          latestRef.current.set(name, {
            memoryUsed: raw.memory_used_bytes,
            memoryLimit: raw.memory_limit_bytes,
          });
          if (needsImmediateFlush.current) {
            needsImmediateFlush.current = false;
            setMemoryMap(new Map(latestRef.current));
          }
        } catch { /* ignore */ }
      });

      ws.addEventListener('close', () => {
        current.delete(name);
      });

      current.set(name, ws);
    }

    const timer = setInterval(() => {
      setMemoryMap(new Map(latestRef.current));
    }, FLUSH_INTERVAL);

    return () => {
      clearInterval(timer);
      for (const ws of current.values()) {
        if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
          ws.close();
        }
      }
      current.clear();
      latestRef.current.clear();
    };
  }, [project, runningNames, streamPath, entityParam]);

  const totalMemory = useMemo(() => {
    let sum = 0;
    for (const info of memoryMap.values()) sum += info.memoryUsed;
    return sum;
  }, [memoryMap]);

  return { memoryMap, totalMemory };
}
