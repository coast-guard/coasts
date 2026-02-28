import { useState, useRef, useEffect, useCallback, type ReactElement, type CSSProperties, type MouseEvent as ReactMouseEvent } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import '@xterm/xterm/css/xterm.css';
import { Plus, X, CornersOut, CornersIn, Microphone, MicrophoneSlash, CaretDown } from '@phosphor-icons/react';
import type { PersistentTerminalConfig } from '../hooks/useTerminalSessions';
import { useTerminalTheme } from '../hooks/useTerminalTheme';
import { api } from '../api/endpoints';
import TerminalThemePicker from './TerminalThemePicker';

const RESIZE_PREFIX = '\x01';
const MAX_TITLE_LEN = 30;

interface SessionState {
  readonly clientId: number;
  serverId: string | null;
  label: string;
  readonly existingId: string | null;
  readonly isAgent: boolean;
  readonly agentShellId: number | null;
  readonly isActiveAgent: boolean;
}

interface TerminalHandle {
  terminal: Terminal;
  fitAddon: FitAddon;
  ws: WebSocket;
  resizeObserver: ResizeObserver;
  receivedSessionId: boolean;
}

interface Props {
  readonly config: PersistentTerminalConfig;
}

function formatAgentTabLabel(shellLabel: string | null | undefined, agentLabel: string): string {
  if (shellLabel == null || shellLabel.trim() === '') return agentLabel;
  return `${shellLabel} | ${agentLabel}`;
}

function parseExecTerminalConfigKey(configKey: string): { project: string; name: string } | null {
  if (!configKey.startsWith('exec:')) return null;
  const rest = configKey.slice('exec:'.length);
  const divider = rest.indexOf(':');
  if (divider <= 0 || divider >= rest.length - 1) return null;
  return {
    project: rest.slice(0, divider),
    name: rest.slice(divider + 1),
  };
}

export default function PersistentTerminal({ config }: Props): ReactElement {
  const { t } = useTranslation();
  const { activeTheme, setTerminalTheme, themes } = useTerminalTheme();
  const [sessions, setSessions] = useState<SessionState[]>([]);
  const [activeClientId, setActiveClientId] = useState<number | null>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const [isRecording, setIsRecording] = useState(false);
  const [spawnMenuOpen, setSpawnMenuOpen] = useState(false);
  const [spawnMenuPos, setSpawnMenuPos] = useState({ top: 0, right: 0 });
  const [agentActionMenu, setAgentActionMenu] = useState<{ clientId: number; top: number; right: number } | null>(null);
  const recognitionRef = useRef<SpeechRecognition | null>(null);
  const addButtonRef = useRef<HTMLButtonElement>(null);
  const spawnMenuRef = useRef<HTMLDivElement>(null);
  const agentActionMenuRef = useRef<HTMLDivElement>(null);
  const nextIdRef = useRef(1);
  const handlesRef = useRef(new Map<number, TerminalHandle>());
  const initedRef = useRef(new Set<number>());
  const configRef = useRef(config);
  configRef.current = config;

  const termBg = activeTheme.colors.background;
  const terminalSurfaceStyle = { background: termBg, '--term-bg': termBg } as CSSProperties;

  const fitAll = useCallback(() => {
    for (const handle of handlesRef.current.values()) {
      handle.fitAddon.fit();
    }
  }, []);

  const debouncedFitAll = useCallback(() => {
    requestAnimationFrame(() => {
      fitAll();
      setTimeout(() => fitAll(), 50);
      setTimeout(() => fitAll(), 150);
    });
  }, [fitAll]);

  const toggleFullscreen = useCallback(() => {
    setFullscreen((prev) => !prev);
  }, []);

  const speechSupported = typeof window !== 'undefined' && ('SpeechRecognition' in window || 'webkitSpeechRecognition' in window);

  const toggleRecording = useCallback(() => {
    if (isRecording) {
      recognitionRef.current?.stop();
      return;
    }

    const SpeechRecognitionCtor = window.SpeechRecognition ?? window.webkitSpeechRecognition;
    if (SpeechRecognitionCtor == null) return;

    const recognition = new SpeechRecognitionCtor();
    recognition.continuous = false;
    recognition.interimResults = false;
    recognition.lang = navigator.language || 'en-US';
    recognitionRef.current = recognition;

    const targetClientId = activeClientId;
    const transcriptParts: string[] = [];
    let flushed = false;

    const flushTranscript = () => {
      if (flushed) return;
      flushed = true;
      if (targetClientId == null) return;
      const handle = handlesRef.current.get(targetClientId);
      if (handle?.ws.readyState !== WebSocket.OPEN) return;
      const transcript = transcriptParts.join(' ').trim();
      if (transcript.length > 0) {
        handle.ws.send(transcript);
      }
    };

    recognition.onresult = (event: SpeechRecognitionEvent) => {
      for (let i = event.resultIndex; i < event.results.length; i++) {
        if (!event.results[i]!.isFinal) continue;
        const text = event.results[i]![0]!.transcript.trim();
        if (text.length > 0) {
          transcriptParts.push(text);
        }
      }
    };

    recognition.onend = () => {
      flushTranscript();
      setIsRecording(false);
      recognitionRef.current = null;
    };

    recognition.onerror = () => {
      flushTranscript();
      setIsRecording(false);
      recognitionRef.current = null;
    };

    recognition.start();
    setIsRecording(true);
  }, [isRecording, activeClientId]);

  useEffect(() => {
    if (!fullscreen) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') setFullscreen(false);
    }
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [fullscreen]);

  useEffect(() => {
    debouncedFitAll();
  }, [fullscreen, debouncedFitAll]);

  const addSession = useCallback((
    existingId: string | null,
    customLabel?: string,
    agentMeta?: { agentShellId: number; isActiveAgent: boolean },
  ) => {
    const clientId = nextIdRef.current++;
    const label = customLabel ?? t('terminal.shell', { id: clientId });
    setSessions((prev) => [...prev, {
      clientId,
      serverId: existingId,
      label,
      existingId,
      isAgent: agentMeta != null,
      agentShellId: agentMeta?.agentShellId ?? null,
      isActiveAgent: agentMeta?.isActiveAgent ?? false,
    }]);
    setActiveClientId(clientId);
    return clientId;
  }, [t]);

  const updateSpawnMenuPosition = useCallback(() => {
    if (addButtonRef.current == null) return;
    const rect = addButtonRef.current.getBoundingClientRect();
    setSpawnMenuPos({
      top: rect.bottom + 6,
      right: window.innerWidth - rect.right,
    });
  }, []);

  const handleSpawnShellTab = useCallback(() => {
    setSpawnMenuOpen(false);
    addSession(null);
  }, [addSession]);

  const refreshAgentShellBadges = useCallback(async () => {
    try {
      const res = await fetch(configRef.current.listSessionsUrl);
      if (!res.ok) return;
      const existing = (await res.json()) as Array<{
        id: string;
        agent_shell_id?: number | null;
        is_active_agent?: boolean | null;
      }>;
      const byServerId = new Map(existing.map((item) => [item.id, item]));
      setSessions((prev) => prev.map((session) => {
        if (!session.isAgent || session.serverId == null) return session;
        const server = byServerId.get(session.serverId);
        if (server?.agent_shell_id == null) return session;
        const nextIsActive = server.is_active_agent === true;
        if (session.agentShellId === server.agent_shell_id && session.isActiveAgent === nextIsActive) {
          return session;
        }
        return {
          ...session,
          agentShellId: server.agent_shell_id,
          isActiveAgent: nextIsActive,
        };
      }));
    } catch {
      // Best effort refresh only; keep current badges if fetch fails.
    }
  }, []);

  const handleSpawnAgentShellTab = useCallback(async () => {
    setSpawnMenuOpen(false);
    const execConfig = parseExecTerminalConfigKey(configRef.current.configKey);
    if (execConfig == null) return;
    try {
      const resp = await api.spawnAgentShell(execConfig.project, execConfig.name);
      const agentMeta = {
        agentShellId: resp.agent_shell_id,
        isActiveAgent: resp.is_active_agent === true,
      };
      const label = formatAgentTabLabel(
        resp.title ?? undefined,
        t('terminal.agent', { id: agentMeta.agentShellId }),
      );
      addSession(resp.session_id, label, agentMeta);
      void refreshAgentShellBadges();
    } catch {
      // Keep the current tab as-is when spawning fails.
    }
  }, [addSession, refreshAgentShellBadges, t]);

  const openAgentActionMenu = useCallback((event: ReactMouseEvent<HTMLButtonElement>, clientId: number) => {
    event.stopPropagation();
    const rect = event.currentTarget.getBoundingClientRect();
    setAgentActionMenu((prev) => (
      prev?.clientId === clientId
        ? null
        : {
          clientId,
          top: rect.bottom + 6,
          right: window.innerWidth - rect.right,
        }
    ));
  }, []);

  const handleMakeActiveAgent = useCallback(async () => {
    if (agentActionMenu == null) return;
    const target = sessions.find((s) => s.clientId === agentActionMenu.clientId);
    if (target == null || !target.isAgent || target.agentShellId == null) {
      setAgentActionMenu(null);
      return;
    }
    const execConfig = parseExecTerminalConfigKey(configRef.current.configKey);
    if (execConfig == null) {
      setAgentActionMenu(null);
      return;
    }
    try {
      await api.activateAgentShell(execConfig.project, execConfig.name, target.agentShellId);
      setAgentActionMenu(null);
      void refreshAgentShellBadges();
    } catch {
      setAgentActionMenu(null);
    }
  }, [agentActionMenu, refreshAgentShellBadges, sessions]);

  const handleCloseAgent = useCallback(async () => {
    if (agentActionMenu == null) return;
    const target = sessions.find((s) => s.clientId === agentActionMenu.clientId);
    if (target == null || !target.isAgent || target.agentShellId == null) {
      setAgentActionMenu(null);
      return;
    }
    const execConfig = parseExecTerminalConfigKey(configRef.current.configKey);
    if (execConfig == null) {
      setAgentActionMenu(null);
      return;
    }
    try {
      await api.closeAgentShell(execConfig.project, execConfig.name, target.agentShellId);
      setAgentActionMenu(null);
      const handle = handlesRef.current.get(target.clientId);
      if (handle != null) {
        handle.resizeObserver.disconnect();
        handle.ws.close();
        handle.terminal.dispose();
        handlesRef.current.delete(target.clientId);
      }
      initedRef.current.delete(target.clientId);
      setSessions((prev) => prev.filter((s) => s.clientId !== target.clientId));
      setActiveClientId((prev) => (prev === target.clientId ? null : prev));
      void refreshAgentShellBadges();
    } catch {
      setAgentActionMenu(null);
    }
  }, [agentActionMenu, refreshAgentShellBadges, sessions]);

  const handleNewTerminalClick = useCallback(async () => {
    const execConfig = parseExecTerminalConfigKey(configRef.current.configKey);
    if (execConfig == null) {
      addSession(null);
      return;
    }

    let available = false;
    try {
      const resp = await api.agentShellAvailable(execConfig.project, execConfig.name);
      available = resp.available;
    } catch { /* keep available = false */ }

    if (!available) {
      addSession(null);
      return;
    }

    updateSpawnMenuPosition();
    setSpawnMenuOpen((prev) => !prev);
  }, [addSession, updateSpawnMenuPosition]);

  useEffect(() => {
    if (!spawnMenuOpen) return;
    updateSpawnMenuPosition();
    function onClickOutside(e: MouseEvent) {
      if (
        spawnMenuRef.current != null && !spawnMenuRef.current.contains(e.target as Node) &&
        addButtonRef.current != null && !addButtonRef.current.contains(e.target as Node)
      ) {
        setSpawnMenuOpen(false);
      }
    }
    function onWindowResize() {
      updateSpawnMenuPosition();
    }
    document.addEventListener('mousedown', onClickOutside);
    window.addEventListener('resize', onWindowResize);
    return () => {
      document.removeEventListener('mousedown', onClickOutside);
      window.removeEventListener('resize', onWindowResize);
    };
  }, [spawnMenuOpen, updateSpawnMenuPosition]);

  useEffect(() => {
    setSpawnMenuOpen(false);
    setAgentActionMenu(null);
  }, [config.configKey]);

  useEffect(() => {
    if (agentActionMenu == null) return;
    function onClickOutside(e: MouseEvent) {
      const target = e.target;
      if (target == null) {
        setAgentActionMenu(null);
        return;
      }
      if (agentActionMenuRef.current != null && agentActionMenuRef.current.contains(target as Node)) {
        return;
      }
      if (target instanceof Element && target.closest('[data-agent-action-caret]') != null) {
        // Let the caret onClick toggle logic handle open/close.
        return;
      }
      setAgentActionMenu(null);
    }
    function onWindowResize() {
      setAgentActionMenu(null);
    }
    document.addEventListener('mousedown', onClickOutside);
    window.addEventListener('resize', onWindowResize);
    return () => {
      document.removeEventListener('mousedown', onClickOutside);
      window.removeEventListener('resize', onWindowResize);
    };
  }, [agentActionMenu]);

  useEffect(() => {
    if (agentActionMenu == null) return;
    const menuTargetExists = sessions.some((s) => s.clientId === agentActionMenu.clientId && s.isAgent && !s.isActiveAgent);
    if (!menuTargetExists) {
      setAgentActionMenu(null);
    }
  }, [agentActionMenu, sessions]);

  const removeSession = useCallback((clientId: number) => {
    const handle = handlesRef.current.get(clientId);
    if (handle != null) {
      handle.resizeObserver.disconnect();
      handle.ws.close();
      handle.terminal.dispose();
      handlesRef.current.delete(clientId);
    }
    initedRef.current.delete(clientId);

    setSessions((prev) => {
      const idx = prev.findIndex((s) => s.clientId === clientId);
      const next = prev.filter((s) => s.clientId !== clientId);
      const session = prev[idx];
      if (session?.serverId != null) {
        void fetch(configRef.current.deleteSessionUrl(session.serverId), { method: 'DELETE' });
      }
      return next;
    });

    setActiveClientId((prev) => {
      if (prev !== clientId) return prev;
      return null;
    });
  }, []);

  useEffect(() => {
    if (activeClientId == null && sessions.length > 0) {
      setActiveClientId(sessions[0]!.clientId);
    }
  }, [activeClientId, sessions]);

  useEffect(() => {
    for (const session of sessions) {
      if (initedRef.current.has(session.clientId)) continue;
      const container = document.getElementById(`term-${session.clientId}`);
      if (container == null) continue;
      initedRef.current.add(session.clientId);

      const terminal = new Terminal({
        cursorBlink: true,
        fontSize: 13,
        fontFamily: '"JetBrains Mono", Menlo, Monaco, "Courier New", monospace',
        theme: activeTheme.colors,
        scrollback: 10000,
        allowProposedApi: true,
        macOptionIsMeta: false,
        macOptionClickForcesSelection: true,
      });

      const fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon);
      terminal.loadAddon(new WebLinksAddon());
      terminal.open(container);

      const ws = new WebSocket(configRef.current.wsUrl(session.existingId));

      const handle: TerminalHandle = {
        terminal,
        fitAddon,
        ws,
        resizeObserver: new ResizeObserver(() => fitAddon.fit()),
        receivedSessionId: false,
      };

      handle.resizeObserver.observe(container);
      handlesRef.current.set(session.clientId, handle);

      const clientId = session.clientId;

      ws.addEventListener('open', () => {
        fitAddon.fit();
        sendResize(ws, terminal.cols, terminal.rows);
      });

      ws.addEventListener('message', (event: MessageEvent<string>) => {
        if (!handle.receivedSessionId) {
          try {
            const parsed: unknown = JSON.parse(event.data);
            if (
              parsed != null &&
              typeof parsed === 'object' &&
              'session_id' in parsed &&
              typeof (parsed as Record<string, unknown>)['session_id'] === 'string'
            ) {
              const sid = (parsed as Record<string, string>)['session_id']!;
              handle.receivedSessionId = true;
              setSessions((prev) =>
                prev.map((s) => (s.clientId === clientId ? { ...s, serverId: sid } : s)),
              );
              return;
            }
          } catch { /* not JSON */ }
          handle.receivedSessionId = true;
        }
        terminal.write(event.data);
      });

      ws.addEventListener('close', () => {
        terminal.write('\r\n\x1b[90m[session ended]\x1b[0m\r\n');
      });

      ws.addEventListener('error', () => {
        terminal.write('\r\n\x1b[31m[connection error]\x1b[0m\r\n');
      });

      terminal.onData((data: string) => {
        if (ws.readyState === WebSocket.OPEN) ws.send(data);
      });

      terminal.attachCustomKeyEventHandler((e: KeyboardEvent) => {
        // Suppress browser defaults for Ctrl combos on all event types
        if (e.ctrlKey && !e.metaKey && !e.altKey) {
          e.preventDefault();
        }

        if (e.type !== 'keydown') return true;

        // Cmd+K: clear terminal + clear daemon scrollback
        if (e.metaKey && e.key === 'k') {
          terminal.clear();
          if (ws.readyState === WebSocket.OPEN) ws.send('\x02clear');
          return false;
        }

        // Let Cmd+C/V through to browser for copy/paste
        if (e.metaKey && (e.key === 'c' || e.key === 'v')) {
          return true;
        }

        // Ctrl+A: beginning of line — send manually to bypass browser "Select All"
        if (e.ctrlKey && (e.key === 'a' || e.key === 'A' || e.code === 'KeyA')) {
          if (ws.readyState === WebSocket.OPEN) ws.send('\x01');
          return false;
        }

        // Ctrl+E: end of line
        if (e.ctrlKey && (e.key === 'e' || e.key === 'E' || e.code === 'KeyE')) {
          if (ws.readyState === WebSocket.OPEN) ws.send('\x05');
          return false;
        }

        // Option+Left: word back (\x1bb)
        if (e.altKey && e.key === 'ArrowLeft') {
          e.preventDefault();
          if (ws.readyState === WebSocket.OPEN) ws.send('\x1bb');
          return false;
        }

        // Option+Right: word forward (\x1bf)
        if (e.altKey && e.key === 'ArrowRight') {
          e.preventDefault();
          if (ws.readyState === WebSocket.OPEN) ws.send('\x1bf');
          return false;
        }

        // Option+Backspace: delete word back (\x17 = Ctrl+W)
        if (e.altKey && e.key === 'Backspace') {
          e.preventDefault();
          if (ws.readyState === WebSocket.OPEN) ws.send('\x17');
          return false;
        }

        // Option+D: delete word forward (\x1bd)
        if (e.altKey && e.key === 'd') {
          e.preventDefault();
          if (ws.readyState === WebSocket.OPEN) ws.send('\x1bd');
          return false;
        }

        return true;
      });

      terminal.onResize(({ cols, rows }) => sendResize(ws, cols, rows));

      terminal.options.allowProposedApi = true;
      terminal.onTitleChange((title: string) => {
        const truncated = title.length > MAX_TITLE_LEN ? title.slice(0, MAX_TITLE_LEN) + '...' : title;
        setSessions((prev) =>
          prev.map((s) => {
            if (s.clientId !== clientId) return s;
            if (s.isAgent && s.agentShellId != null) {
              return { ...s, label: formatAgentTabLabel(truncated, t('terminal.agent', { id: s.agentShellId })) };
            }
            return { ...s, label: truncated };
          }),
        );
        setSessions((prevSessions) => {
          const sess = prevSessions.find((s) => s.clientId === clientId);
          if (sess?.serverId != null) {
            void api.setSetting(`session_title:${sess.serverId}`, truncated);
          }
          return prevSessions;
        });
      });

      container.addEventListener('dragover', (e: DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        if (e.dataTransfer != null) e.dataTransfer.dropEffect = 'copy';
      });

      container.addEventListener('drop', (e: DragEvent) => {
        e.preventDefault();
        e.stopPropagation();
        void handleFileDrop(e, ws, configRef.current);
      });
    }
  }, [sessions]);

  useEffect(() => {
    if (activeClientId == null) return;
    const handle = handlesRef.current.get(activeClientId);
    if (handle == null) return;
    requestAnimationFrame(() => {
      handle.fitAddon.fit();
      handle.terminal.focus();
    });
  }, [activeClientId]);

  // Sync terminal theme when activeTheme changes (user picks new theme or app theme toggles)
  useEffect(() => {
    for (const handle of handlesRef.current.values()) {
      handle.terminal.options.theme = activeTheme.colors;
      handle.fitAddon.fit();
    }
  }, [activeTheme]);

  // Persist tab order when sessions change
  useEffect(() => {
    const ids = sessions.filter((s) => s.serverId != null).map((s) => s.serverId!);
    if (ids.length > 0) {
      void api.setSetting(`tab_order:${configRef.current.configKey}`, JSON.stringify(ids));
    }
  }, [sessions]);

  // Persist active tab when it changes
  useEffect(() => {
    if (activeClientId == null) return;
    const sess = sessions.find((s) => s.clientId === activeClientId);
    if (sess?.serverId != null) {
      void api.setSetting(`tab_active:${configRef.current.configKey}`, sess.serverId);
    }
  }, [activeClientId, sessions]);

  useEffect(() => {
    return () => {
      recognitionRef.current?.stop();
      for (const handle of handlesRef.current.values()) {
        handle.resizeObserver.disconnect();
        handle.ws.close();
        handle.terminal.dispose();
      }
      handlesRef.current.clear();
      initedRef.current.clear();
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const res = await fetch(config.listSessionsUrl);
        if (cancelled) return;
        if (res.ok) {
          const existing = (await res.json()) as Array<{
            id: string;
            title?: string | null;
            agent_shell_id?: number | null;
            is_active_agent?: boolean | null;
          }>;
          if (existing.length > 0) {
            const orderJson = await api.getSetting(`tab_order:${config.configKey}`);
            const order: string[] = orderJson != null ? JSON.parse(orderJson) as string[] : [];

            const sorted = [...existing].sort((a, b) => {
              const aIsAgent = a.agent_shell_id != null;
              const bIsAgent = b.agent_shell_id != null;
              if (aIsAgent !== bIsAgent) return aIsAgent ? -1 : 1;
              const ia = order.indexOf(a.id);
              const ib = order.indexOf(b.id);
              if (ia === -1 && ib === -1) return 0;
              if (ia === -1) return 1;
              if (ib === -1) return -1;
              return ia - ib;
            });

            const clientIds: number[] = [];
            for (const sess of sorted) {
              const agentMeta = sess.agent_shell_id != null
                ? { agentShellId: sess.agent_shell_id, isActiveAgent: sess.is_active_agent === true }
                : undefined;
              const label = agentMeta != null
                ? formatAgentTabLabel(sess.title ?? undefined, t('terminal.agent', { id: agentMeta.agentShellId }))
                : sess.title ?? undefined;
              const cid = addSession(sess.id, label, agentMeta);
              clientIds.push(cid);
            }

            const activeId = await api.getSetting(`tab_active:${config.configKey}`);
            if (activeId != null && !cancelled) {
              const targetSession = sorted.find((s) => s.id === activeId);
              if (targetSession != null) {
                const idx = sorted.indexOf(targetSession);
                if (idx >= 0 && idx < clientIds.length) {
                  setActiveClientId(clientIds[idx]!);
                }
              }
            }
          } else {
            addSession(null);
          }
        } else {
          addSession(null);
        }
      } catch {
        if (!cancelled) addSession(null);
      }
    })();
    return () => { cancelled = true; };
  }, []);

  const syncNewAgentSessions = useCallback(async () => {
    try {
      const res = await fetch(configRef.current.listSessionsUrl);
      if (!res.ok) return;
      const serverSessions = (await res.json()) as Array<{
        id: string;
        title?: string | null;
        agent_shell_id?: number | null;
        is_active_agent?: boolean | null;
      }>;
      const byServerId = new Map(serverSessions.map((s) => [s.id, s]));
      setSessions((prev) => {
        const knownServerIds = new Set(prev.map((s) => s.serverId).filter(Boolean));

        const updated = prev.map((session) => {
          if (!session.isAgent || session.serverId == null) return session;
          const server = byServerId.get(session.serverId);
          if (server?.agent_shell_id == null) return session;
          const nextIsActive = server.is_active_agent === true;
          if (session.isActiveAgent === nextIsActive) return session;
          return { ...session, isActiveAgent: nextIsActive };
        });

        const newAgents = serverSessions.filter(
          (s) => s.agent_shell_id != null && !knownServerIds.has(s.id),
        );
        if (newAgents.length === 0) return updated;

        const additions = newAgents.map((sess) => {
          const cid = nextIdRef.current++;
          return {
            clientId: cid,
            serverId: sess.id,
            label: formatAgentTabLabel(sess.title ?? undefined, t('terminal.agent', { id: sess.agent_shell_id! })),
            existingId: sess.id,
            isAgent: true,
            agentShellId: sess.agent_shell_id!,
            isActiveAgent: sess.is_active_agent === true,
          };
        });
        return [...updated, ...additions];
      });
    } catch { /* best effort */ }
  }, [t]);

  useEffect(() => {
    const handler = () => void syncNewAgentSessions();
    window.addEventListener('coast:agent-shell-changed', handler);
    return () => window.removeEventListener('coast:agent-shell-changed', handler);
  }, [syncNewAgentSessions]);

  const iconBtn = 'h-8 w-8 inline-flex items-center justify-center rounded-lg text-subtle-ui hover:text-main hover:bg-white/25 dark:hover:bg-white/10 transition-colors shrink-0';

  return (
    <div
      className={
        fullscreen
          ? 'fixed inset-0 z-[200] flex flex-col max-h-screen overflow-hidden'
          : 'glass-panel flex flex-col'
      }
      style={fullscreen ? terminalSurfaceStyle : { ['--term-bg' as const]: termBg } as CSSProperties}
    >
      {/* Tab bar */}
      <div className="flex items-center h-10 min-h-[2.5rem] shrink-0 px-4 bg-white/20 dark:bg-white/6 border-b border-[var(--border)] overflow-hidden">
        <div className="flex items-center overflow-x-auto flex-1 min-w-0 h-full">
          {sessions.map((s) => (
            <div
              key={s.clientId}
              onClick={() => setActiveClientId(s.clientId)}
              className={`inline-flex items-center gap-1.5 h-7 px-3 text-xs font-semibold cursor-pointer select-none rounded-md transition-colors shrink-0 whitespace-nowrap ${
                s.clientId === activeClientId
                  ? 'bg-white/35 dark:bg-white/12 text-main'
                  : 'text-subtle-ui hover:text-main hover:bg-white/15 dark:hover:bg-white/8'
              }`}
            >
              {s.isAgent && (
                <span
                  className={`h-1.5 w-1.5 rounded-full shrink-0 ${
                    s.isActiveAgent ? 'bg-emerald-500' : 'bg-amber-400'
                  }`}
                />
              )}
              <span className="leading-none">{s.label}</span>
              {s.isAgent && !s.isActiveAgent && (
                <button
                  onClick={(e) => openAgentActionMenu(e, s.clientId)}
                  data-agent-action-caret={s.clientId}
                  className="inline-flex items-center justify-center h-4 w-4 rounded text-subtle-ui hover:text-main hover:bg-white/15 dark:hover:bg-white/8 transition-colors"
                  title={t('terminal.agentActions')}
                >
                  <CaretDown size={10} />
                </button>
              )}
              {!s.isAgent && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    removeSession(s.clientId);
                  }}
                  className="inline-flex items-center justify-center h-4 w-4 rounded text-subtle-ui hover:text-rose-500 hover:bg-rose-500/12 transition-colors"
                >
                  <X size={10} />
                </button>
              )}
            </div>
          ))}
        </div>
        <div className="inline-flex items-center gap-1 pl-2 ml-2 shrink-0 border-l border-[var(--border)]">
          {speechSupported && (
            <button
              onClick={toggleRecording}
              className={`${iconBtn} ${isRecording ? '!text-red-500 animate-pulse' : ''}`}
              title={isRecording ? t('terminal.stopRecording') : t('terminal.startRecording')}
            >
              {isRecording ? <MicrophoneSlash size={18} /> : <Microphone size={18} />}
            </button>
          )}
          <button
            ref={addButtonRef}
            onClick={() => { void handleNewTerminalClick(); }}
            className={iconBtn}
            title={t('terminal.newTerminal')}
          >
            <Plus size={18} />
          </button>
          <TerminalThemePicker
            themes={themes}
            activeId={activeTheme.id}
            onSelect={setTerminalTheme}
          />
          <button
            onClick={toggleFullscreen}
            className={iconBtn}
            title={fullscreen ? t('terminal.exitFullscreen') : t('terminal.fullscreen')}
          >
            {fullscreen ? <CornersIn size={18} /> : <CornersOut size={18} />}
          </button>
        </div>
      </div>

      {/* Terminal panels */}
      <div
        className={fullscreen ? 'flex-1 min-h-0 overflow-hidden' : 'overflow-hidden rounded-b-[var(--radius-lg)]'}
        style={terminalSurfaceStyle}
      >
        {sessions.map((s) => (
          <div
            key={s.clientId}
            id={`term-${s.clientId}`}
            className={fullscreen
              ? `${s.clientId === activeClientId ? 'w-full h-full overflow-hidden' : 'hidden'}`
              : `xterm-container ${s.clientId === activeClientId ? '' : 'hidden'}`
            }
          />
        ))}
      </div>
      {agentActionMenu != null && createPortal(
        <div
          ref={agentActionMenuRef}
          className="fixed glass-panel py-1 min-w-[190px] z-[310]"
          style={{ top: agentActionMenu.top, right: agentActionMenu.right }}
        >
          <button
            onClick={() => { void handleMakeActiveAgent(); }}
            className="w-full text-left px-3 py-2 text-sm text-muted-ui hover:text-main hover:bg-white/15 dark:hover:bg-white/8 transition-colors"
          >
            {t('terminal.makeActiveAgent')}
          </button>
          <button
            onClick={() => { void handleCloseAgent(); }}
            className="w-full text-left px-3 py-2 text-sm text-muted-ui hover:text-main hover:bg-white/15 dark:hover:bg-white/8 transition-colors"
          >
            {t('terminal.closeAgent')}
          </button>
        </div>,
        document.body,
      )}
      {spawnMenuOpen && createPortal(
        <div
          ref={spawnMenuRef}
          className="fixed glass-panel py-1 min-w-[180px] z-[300]"
          style={{ top: spawnMenuPos.top, right: spawnMenuPos.right }}
        >
          <button
            onClick={handleSpawnShellTab}
            className="w-full text-left px-3 py-2 text-sm text-muted-ui hover:text-main hover:bg-white/15 dark:hover:bg-white/8 transition-colors"
          >
            {t('terminal.newShell')}
          </button>
          <button
            onClick={() => { void handleSpawnAgentShellTab(); }}
            className="w-full text-left px-3 py-2 text-sm text-muted-ui hover:text-main hover:bg-white/15 dark:hover:bg-white/8 transition-colors"
          >
            {t('terminal.newAgentShell')}
          </button>
        </div>,
        document.body,
      )}
    </div>
  );
}

async function handleFileDrop(
  e: DragEvent,
  ws: WebSocket,
  cfg: { readonly uploadUrl: string | null; readonly uploadMeta: { readonly project: string; readonly name: string } | null },
): Promise<void> {
  if (ws.readyState !== WebSocket.OPEN) return;

  const files = e.dataTransfer?.files;
  if (files != null && files.length > 0 && cfg.uploadUrl != null) {
    const paths: string[] = [];
    for (let i = 0; i < files.length; i++) {
      const file = files[i]!;
      const form = new FormData();
      if (cfg.uploadMeta != null) {
        form.append('project', cfg.uploadMeta.project);
        form.append('name', cfg.uploadMeta.name);
      }
      form.append('file', file);
      try {
        const res = await fetch(cfg.uploadUrl, { method: 'POST', body: form });
        if (res.ok) {
          const json = (await res.json()) as { path: string };
          paths.push(shellEscape(json.path));
        }
      } catch { /* upload failed */ }
    }
    if (paths.length > 0) {
      ws.send(paths.join(' '));
    }
    return;
  }

  const localPaths = extractLocalPaths(e);
  if (localPaths.length > 0) {
    ws.send(localPaths.join(' '));
  }
}

function extractLocalPaths(e: DragEvent): string[] {
  const results: string[] = [];
  for (const src of [e.dataTransfer?.getData('text/uri-list'), e.dataTransfer?.getData('text/plain')]) {
    if (src == null || src.length === 0) continue;
    for (const line of src.split('\n')) {
      const trimmed = line.trim();
      if (trimmed.startsWith('file://')) {
        results.push(shellEscape(decodeURIComponent(trimmed.slice(7))));
      } else if (trimmed.startsWith('/')) {
        results.push(shellEscape(trimmed));
      }
    }
    if (results.length > 0) break;
  }
  return results;
}

function shellEscape(path: string): string {
  if (/^[a-zA-Z0-9_./-]+$/.test(path)) return path;
  return "'" + path.replace(/'/g, "'\\''") + "'";
}

function sendResize(ws: WebSocket, cols: number, rows: number): void {
  if (ws.readyState === WebSocket.OPEN) {
    ws.send(RESIZE_PREFIX + JSON.stringify({ cols, rows }));
  }
}
