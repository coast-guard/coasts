import { useMemo, useState, useEffect, useRef } from 'react';
import type { ProjectName, InstanceName } from '../types/branded';
import { buildExecTerminalConfig } from '../hooks/useTerminalSessions';
import PersistentTerminal from '../components/PersistentTerminal';

interface Props {
  readonly project: ProjectName;
  readonly name: InstanceName;
}

export default function InstanceExecTab({ project, name }: Props) {
  const config = useMemo(
    () => buildExecTerminalConfig(project, name),
    [project, name],
  );

  const [staleAssign, setStaleAssign] = useState(false);
  const assignCountRef = useRef(0);

  useEffect(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${protocol}//${window.location.host}/api/v1/events`;
    let ws: WebSocket;
    let timer: ReturnType<typeof setTimeout>;

    function connect() {
      ws = new WebSocket(url);
      ws.addEventListener('message', (event: MessageEvent<string>) => {
        try {
          const parsed = JSON.parse(event.data) as Record<string, unknown>;
          if (
            parsed['event'] === 'instance.assigned' &&
            parsed['name'] === name &&
            parsed['project'] === project
          ) {
            assignCountRef.current += 1;
            if (assignCountRef.current > 0) {
              setStaleAssign(true);
            }
          }
        } catch { /* ignore parse errors */ }
      });
      ws.addEventListener('close', () => { timer = setTimeout(connect, 3000); });
      ws.addEventListener('error', () => ws.close());
    }
    connect();
    return () => {
      clearTimeout(timer);
      try { ws.close(); } catch { /* ignore */ }
    };
  }, [project, name]);

  return (
    <div className="flex flex-col h-full">
      {staleAssign && (
        <div className="flex items-center gap-3 px-4 py-2 bg-amber-900/30 border-b border-amber-700/50 text-amber-200 text-sm shrink-0">
          <span>Worktree changed. Existing shells have a stale working directory.</span>
          <button
            className="px-3 py-1 rounded bg-amber-700/50 hover:bg-amber-700/80 text-amber-100 text-xs font-medium transition-colors"
            onClick={() => {
              setStaleAssign(false);
              assignCountRef.current = 0;
            }}
          >
            Dismiss
          </button>
          <span className="text-amber-400/70 text-xs">Open a new shell tab to use the updated worktree.</span>
        </div>
      )}
      <div className="flex-1 min-h-0">
        <PersistentTerminal config={config} />
      </div>
    </div>
  );
}
