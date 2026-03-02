import { useEffect, useRef, useCallback } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useLocation } from 'react-router';
import type { CoastEvent } from '../types/generated/CoastEvent';
import { useServiceOperations } from '../providers/ServiceOperationsProvider';
import { useRemovingProjects } from '../providers/RemovingProjectsProvider';

const GIT_EVENTS = new Set([
  'instance.assigned',
  'instance.unassigned',
  'instance.created',
  'instance.removed',
  'project.git_changed',
]);

export function useCoastEvents(): void {
  const qc = useQueryClient();
  const location = useLocation();
  const locRef = useRef(location);
  locRef.current = location;
  const { setOperation } = useServiceOperations();
  const { addRemoving, removeRemoving, addRemovingBuild, removeRemovingBuild } = useRemovingProjects();

  const handleServiceEvent = useCallback(
    (evt: CoastEvent) => {
      if (!evt.event.startsWith('service.')) return;

      if (!('service' in evt && 'name' in evt)) return;
      const service = (evt as { service: string }).service;
      const name = (evt as { name: string }).name;
      const project = evt.project;

      const key = `${project}:${name}:${service}`;
      const action = evt.event.slice('service.'.length);

      const inProgress = new Set(['stopping', 'starting', 'restarting', 'removing']);
      if (inProgress.has(action)) {
        setOperation(key, { status: action as 'stopping' | 'starting' | 'restarting' | 'removing' });
      } else if (action === 'error') {
        const error = 'error' in evt ? (evt as { error: string }).error : undefined;
        setOperation(key, { status: 'error', error });
        setTimeout(() => setOperation(key, null), 6000);
      } else {
        setOperation(key, { status: action as 'stopped' | 'started' | 'restarted' | 'removed' });
        setTimeout(() => setOperation(key, null), 3000);
      }

      void qc.invalidateQueries({ queryKey: ['services'] });
    },
    [setOperation, qc],
  );

  useEffect(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${protocol}//${window.location.host}/api/v1/events`;
    let ws: WebSocket;
    let timer: ReturnType<typeof setTimeout>;

    function connect() {
      ws = new WebSocket(url);

      ws.addEventListener('message', (message: MessageEvent<string>) => {
        try {
          const evt = JSON.parse(message.data) as CoastEvent;

          if (evt.event.startsWith('service.')) {
            handleServiceEvent(evt);
          }

          if (evt.event === 'build.removing') {
            addRemoving(evt.project);
            if ('build_ids' in evt) {
              for (const bid of evt.build_ids) addRemovingBuild(bid);
            }
          } else if (evt.event === 'build.removed') {
            removeRemoving(evt.project);
            if ('build_ids' in evt) {
              for (const bid of evt.build_ids) removeRemovingBuild(bid);
            }
          }

          if (evt.event === 'build.completed' || evt.event === 'build.removed') {
            void qc.invalidateQueries({ queryKey: ['buildsLs'] });
            void qc.invalidateQueries({ queryKey: ['buildsInspect'] });
            void qc.invalidateQueries({ queryKey: ['buildsImages'] });
            void qc.invalidateQueries({ queryKey: ['buildsDockerImages'] });
            void qc.invalidateQueries({ queryKey: ['buildsCompose'] });
          }

          if (evt.event === 'project.archived' || evt.event === 'project.unarchived') {
            void qc.invalidateQueries({ queryKey: ['sharedServicesAll'] });
          }

          if (evt.event.startsWith('shared_service.')) {
            void qc.invalidateQueries({ queryKey: ['sharedServices'] });
            void qc.invalidateQueries({ queryKey: ['sharedServicesAll'] });
          }

          void qc.invalidateQueries({ queryKey: ['instances'] });

          if (evt.event === 'instance.services_restarted') {
            void qc.invalidateQueries({ queryKey: ['services'] });
          }

          if (
            evt.event === 'instance.status_changed' ||
            evt.event === 'instance.created' ||
            evt.event === 'instance.removed'
          ) {
            void qc.invalidateQueries({ queryKey: ['buildsLs'] });
          }

          if (evt.event === 'port.primary_changed' || evt.event === 'instance.checked_out') {
            if ('project' in evt) {
              void qc.invalidateQueries({ queryKey: ['ports', evt.project] });
            }
          }

          if (GIT_EVENTS.has(evt.event)) {
            void qc.invalidateQueries({ queryKey: ['projectGit'] });
          }

          if (evt.event === 'docker.status_changed') {
            void qc.invalidateQueries({ queryKey: ['dockerInfo'] });
          }

          if (evt.event === 'agent_shell.spawned') {
            window.dispatchEvent(new CustomEvent('coast:agent-shell-changed', { detail: evt }));
          }
        } catch {
          void qc.invalidateQueries({ queryKey: ['instances'] });
        }
      });

      ws.addEventListener('close', () => {
        timer = setTimeout(connect, 3000);
      });

      ws.addEventListener('error', () => ws.close());
    }

    connect();

    return () => {
      clearTimeout(timer);
      if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CLOSING) {
        ws.close();
      } else {
        ws.addEventListener('open', () => ws.close());
      }
    };
  }, [qc, handleServiceEvent, addRemoving, removeRemoving, addRemovingBuild, removeRemovingBuild]);
}
