import { createContext, useContext, useCallback, useState } from 'react';

export type ServiceOpStatus =
  | 'stopping'
  | 'stopped'
  | 'starting'
  | 'started'
  | 'restarting'
  | 'restarted'
  | 'removing'
  | 'removed'
  | 'error';

export interface ServiceOperation {
  readonly status: ServiceOpStatus;
  readonly error?: string | undefined;
}

export type ServiceOperationsMap = ReadonlyMap<string, ServiceOperation>;

export interface ServiceOperationsContextValue {
  readonly operations: ServiceOperationsMap;
  readonly setOperation: (key: string, op: ServiceOperation | null) => void;
}

export const ServiceOperationsContext =
  createContext<ServiceOperationsContextValue>({
    operations: new Map(),
    setOperation: () => {},
  });

export function ServiceOperationsProvider({ children }: { readonly children: React.ReactNode }) {
  const [ops, setOps] = useState<ServiceOperationsMap>(new Map());

  const setOperation = useCallback(
    (key: string, op: ServiceOperation | null) => {
      setOps((prev) => {
        const next = new Map(prev);
        if (op == null) {
          next.delete(key);
        } else {
          next.set(key, op);
        }
        return next;
      });
    },
    [],
  );

  return (
    <ServiceOperationsContext.Provider value={{ operations: ops, setOperation }}>
      {children}
    </ServiceOperationsContext.Provider>
  );
}

export function useServiceOperations(): ServiceOperationsContextValue {
  return useContext(ServiceOperationsContext);
}

export function serviceOpKey(
  project: string,
  name: string,
  service: string,
): string {
  return `${project}:${name}:${service}`;
}

export function useServiceOpForService(
  project: string,
  name: string,
  service: string,
): ServiceOperation | undefined {
  const { operations } = useServiceOperations();
  return operations.get(serviceOpKey(project, name, service));
}

const IN_PROGRESS_STATUSES = new Set<ServiceOpStatus>([
  'stopping',
  'starting',
  'restarting',
  'removing',
]);

export function isInProgress(op: ServiceOperation | undefined): boolean {
  return op != null && IN_PROGRESS_STATUSES.has(op.status);
}

const COMPLETED_CLEAR_DELAY = 3000;

export function useServiceEventHandler(
  setOperation: (key: string, op: ServiceOperation | null) => void,
) {
  return useCallback(
    (event: { type?: string; service?: string; name?: string; project?: string; error?: string }) => {
      const { service, name, project } = event;
      if (service == null || name == null || project == null) return;
      const key = serviceOpKey(project, name, service);
      const type = event.type ?? '';

      if (type.startsWith('service.')) {
        const action = type.slice('service.'.length) as ServiceOpStatus;
        if (IN_PROGRESS_STATUSES.has(action)) {
          setOperation(key, { status: action });
        } else if (action === 'error') {
          setOperation(key, { status: 'error', error: event.error });
          setTimeout(() => setOperation(key, null), COMPLETED_CLEAR_DELAY * 2);
        } else {
          setOperation(key, { status: action });
          setTimeout(() => setOperation(key, null), COMPLETED_CLEAR_DELAY);
        }
      }
    },
    [setOperation],
  );
}
