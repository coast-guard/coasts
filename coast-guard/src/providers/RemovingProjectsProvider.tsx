import { createContext, useContext, useCallback, useState } from 'react';

export interface RemovingProjectsContextValue {
  readonly removing: ReadonlySet<string>;
  readonly addRemoving: (project: string) => void;
  readonly removeRemoving: (project: string) => void;
  readonly removingBuilds: ReadonlySet<string>;
  readonly addRemovingBuild: (buildId: string) => void;
  readonly removeRemovingBuild: (buildId: string) => void;
}

export const RemovingProjectsContext = createContext<RemovingProjectsContextValue>({
  removing: new Set(),
  addRemoving: () => {},
  removeRemoving: () => {},
  removingBuilds: new Set(),
  addRemovingBuild: () => {},
  removeRemovingBuild: () => {},
});

export function RemovingProjectsProvider({ children }: { readonly children: React.ReactNode }) {
  const [removing, setRemoving] = useState<ReadonlySet<string>>(new Set());
  const [removingBuilds, setRemovingBuilds] = useState<ReadonlySet<string>>(new Set());

  const addRemoving = useCallback((project: string) => {
    setRemoving((prev) => {
      const next = new Set(prev);
      next.add(project);
      return next;
    });
  }, []);

  const removeRemoving = useCallback((project: string) => {
    setRemoving((prev) => {
      const next = new Set(prev);
      next.delete(project);
      return next;
    });
  }, []);

  const addRemovingBuild = useCallback((buildId: string) => {
    setRemovingBuilds((prev) => {
      const next = new Set(prev);
      next.add(buildId);
      return next;
    });
  }, []);

  const removeRemovingBuild = useCallback((buildId: string) => {
    setRemovingBuilds((prev) => {
      const next = new Set(prev);
      next.delete(buildId);
      return next;
    });
  }, []);

  return (
    <RemovingProjectsContext.Provider value={{ removing, addRemoving, removeRemoving, removingBuilds, addRemovingBuild, removeRemovingBuild }}>
      {children}
    </RemovingProjectsContext.Provider>
  );
}

export function useRemovingProjects(): RemovingProjectsContextValue {
  return useContext(RemovingProjectsContext);
}
