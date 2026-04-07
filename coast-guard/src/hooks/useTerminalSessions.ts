import { useCallback, useRef } from 'react';
import type { SessionInfo } from '../types/api';

export interface PersistentTerminalConfig {
  readonly listSessionsUrl: string;
  readonly deleteSessionUrl: (id: string) => string;
  readonly wsUrl: (sessionId: string | null) => string;
  readonly uploadUrl: string | null;
  readonly uploadMeta: { readonly project: string; readonly name: string } | null;
  readonly configKey: string;
}

export function buildHostTerminalConfig(project: string): PersistentTerminalConfig {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.host;
  const ep = encodeURIComponent(project);
  return {
    listSessionsUrl: `/api/v1/host/sessions?project=${ep}`,
    deleteSessionUrl: (id) => `/api/v1/host/sessions?id=${encodeURIComponent(id)}`,
    wsUrl: (sid) => {
      let url = `${proto}//${host}/api/v1/host/terminal?project=${ep}`;
      if (sid != null) url += `&session_id=${encodeURIComponent(sid)}`;
      return url;
    },
    uploadUrl: `/api/v1/upload/host`,
    uploadMeta: null,
    configKey: `host:${project}`,
  };
}

export function buildExecTerminalConfig(
  project: string,
  name: string,
): PersistentTerminalConfig {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.host;
  const ep = encodeURIComponent(project);
  const en = encodeURIComponent(name);
  return {
    listSessionsUrl: `/api/v1/exec/sessions?project=${ep}&name=${en}`,
    deleteSessionUrl: (id) => `/api/v1/exec/sessions?id=${encodeURIComponent(id)}`,
    wsUrl: (sid) => {
      let url = `${proto}//${host}/api/v1/exec/interactive?project=${ep}&name=${en}`;
      if (sid != null) url += `&session_id=${encodeURIComponent(sid)}`;
      return url;
    },
    uploadUrl: `/api/v1/upload`,
    uploadMeta: { project, name },
    configKey: `exec:${project}:${name}`,
  };
}

export function buildLocalExecTerminalConfig(
  project: string,
  name: string,
): PersistentTerminalConfig {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.host;
  const ep = encodeURIComponent(project);
  const en = encodeURIComponent(name);
  return {
    listSessionsUrl: `/api/v1/exec/sessions?project=${ep}&name=${en}&local=true`,
    deleteSessionUrl: (id) => `/api/v1/exec/sessions?id=${encodeURIComponent(id)}`,
    wsUrl: (sid) => {
      let url = `${proto}//${host}/api/v1/exec/interactive?project=${ep}&name=${en}&local=true`;
      if (sid != null) url += `&session_id=${encodeURIComponent(sid)}`;
      return url;
    },
    uploadUrl: `/api/v1/upload`,
    uploadMeta: { project, name },
    configKey: `local-exec:${project}:${name}`,
  };
}

export function buildServiceExecTerminalConfig(
  project: string,
  name: string,
  service: string,
): PersistentTerminalConfig {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.host;
  const ep = encodeURIComponent(project);
  const en = encodeURIComponent(name);
  const es = encodeURIComponent(service);
  return {
    listSessionsUrl: `/api/v1/service/sessions?project=${ep}&name=${en}&service=${es}`,
    deleteSessionUrl: (id) => `/api/v1/service/sessions?id=${encodeURIComponent(id)}`,
    wsUrl: (sid) => {
      let url = `${proto}//${host}/api/v1/service/exec?project=${ep}&name=${en}&service=${es}`;
      if (sid != null) url += `&session_id=${encodeURIComponent(sid)}`;
      return url;
    },
    uploadUrl: `/api/v1/upload`,
    uploadMeta: { project, name },
    configKey: `service:${project}:${name}:${service}`,
  };
}

export function buildHostServiceExecTerminalConfig(
  project: string,
  service: string,
): PersistentTerminalConfig {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.host;
  const ep = encodeURIComponent(project);
  const es = encodeURIComponent(service);
  return {
    listSessionsUrl: `/api/v1/host-service/sessions?project=${ep}&service=${es}`,
    deleteSessionUrl: (id) => `/api/v1/host-service/sessions?id=${encodeURIComponent(id)}`,
    wsUrl: (sid) => {
      let url = `${proto}//${host}/api/v1/host-service/exec?project=${ep}&service=${es}`;
      if (sid != null) url += `&session_id=${encodeURIComponent(sid)}`;
      return url;
    },
    uploadUrl: `/api/v1/upload`,
    uploadMeta: { project, name: `host:${service}` },
    configKey: `host-service:${project}:${service}`,
  };
}

export function buildRemoteExecTerminalConfig(
  remoteName: string,
): PersistentTerminalConfig {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const host = window.location.host;
  const en = encodeURIComponent(remoteName);
  const sc = encodeURIComponent(`remote-exec:${remoteName}`);
  return {
    listSessionsUrl: `/api/v1/remote/exec/sessions?name=${en}&scope=${sc}`,
    deleteSessionUrl: (id) => `/api/v1/remote/exec/sessions?id=${encodeURIComponent(id)}`,
    wsUrl: (sid) => {
      let url = `${proto}//${host}/api/v1/remote/exec/interactive?name=${en}&scope=${sc}`;
      if (sid != null) url += `&session_id=${encodeURIComponent(sid)}`;
      return url;
    },
    uploadUrl: null,
    uploadMeta: null,
    configKey: `remote-exec:${remoteName}`,
  };
}

export function useSessionFetcher(listUrl: string) {
  const fetchedRef = useRef(false);

  const fetchSessions = useCallback(async (): Promise<readonly SessionInfo[]> => {
    if (fetchedRef.current) return [];
    fetchedRef.current = true;
    try {
      const res = await fetch(listUrl);
      if (res.ok) return (await res.json()) as SessionInfo[];
    } catch { /* ignore */ }
    return [];
  }, [listUrl]);

  return fetchSessions;
}
