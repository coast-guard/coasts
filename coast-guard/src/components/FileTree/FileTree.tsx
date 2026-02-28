import { useState, useCallback, useImperativeHandle, forwardRef, useEffect, useRef } from 'react';
import { api } from '../../api/endpoints';
import { type FileEntry, type TreeNode, collectExpanded } from './helpers';
import TreeNodeRow from './TreeNodeRow';

interface Props {
  readonly project: string;
  readonly name: string;
  readonly rootPath: string;
  readonly activePath: string | null;
  readonly onFileSelect: (path: string) => void;
  readonly gitStatus?: Map<string, string>;
  readonly isLight?: boolean;
  readonly initialExpanded?: readonly string[] | undefined;
  readonly onExpandedChange?: (paths: string[]) => void;
}

export interface FileTreeHandle {
  revealPath: (path: string) => void;
}

const FileTree = forwardRef<FileTreeHandle, Props>(function FileTree(
  { project, name, rootPath, activePath, onFileSelect, gitStatus, isLight = false, initialExpanded, onExpandedChange }, ref,
) {
  const [nodes, setNodes] = useState<TreeNode[]>([]);
  const [loaded, setLoaded] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const onExpandedChangeRef = useRef(onExpandedChange);
  onExpandedChangeRef.current = onExpandedChange;

  // Tracks whether the tree is ready to report expanded-path changes to the
  // parent. Stays false until the initial expansion (from `initialExpanded` or
  // `activePath` reveal) has been applied to avoid prematurely reporting an
  // empty set that would overwrite the persisted value.
  const [reportReady, setReportReady] = useState(false);

  const loadChildren = useCallback(
    async (path: string): Promise<TreeNode[]> => {
      const entries = await api.fileTree(project, name, path);
      return (entries as FileEntry[]).map((e) => ({
        entry: e, path: `${path}/${e.name}`, children: null, loading: false, expanded: false,
      }));
    },
    [project, name],
  );

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const children = await loadChildren(rootPath);
      if (!cancelled) {
        setNodes(children);
        setLoaded(true);
      }
    })();
    return () => { cancelled = true; };
  }, [loadChildren, rootPath]);

  const updateNode = useCallback(
    (nodes: TreeNode[], targetPath: string, updater: (n: TreeNode) => TreeNode): TreeNode[] => {
      return nodes.map((nd) => {
        if (nd.path === targetPath) return updater(nd);
        if (nd.children != null && targetPath.startsWith(nd.path + '/')) {
          return { ...nd, children: updateNode(nd.children, targetPath, updater) };
        }
        return nd;
      });
    },
    [],
  );

  const findNode = useCallback((nodes: TreeNode[], p: string): TreeNode | undefined => {
    for (const nd of nodes) {
      if (nd.path === p) return nd;
      if (nd.children != null) { const found = findNode(nd.children, p); if (found) return found; }
    }
    return undefined;
  }, []);

  const handleToggle = useCallback(
    async (path: string) => {
      const node = findNode(nodes, path);
      if (node == null) return;
      if (node.expanded) {
        setNodes((prev) => updateNode(prev, path, (nd) => ({ ...nd, expanded: false })));
        return;
      }
      if (node.children != null) {
        setNodes((prev) => updateNode(prev, path, (nd) => ({ ...nd, expanded: true })));
        return;
      }
      setNodes((prev) => updateNode(prev, path, (nd) => ({ ...nd, loading: true })));
      const children = await loadChildren(path);
      setNodes((prev) => updateNode(prev, path, (nd) => ({ ...nd, children, loading: false, expanded: true })));
    },
    [nodes, loadChildren, updateNode, findNode],
  );

  const collapseNonAncestors = useCallback(
    (treeNodes: TreeNode[], targetPath: string): TreeNode[] => {
      return treeNodes.map((nd) => {
        if (nd.entry.type !== 'dir' || !nd.expanded) return nd;
        const isAncestor = targetPath.startsWith(nd.path + '/');
        if (!isAncestor) {
          return { ...nd, expanded: false };
        }
        if (nd.children != null) {
          return { ...nd, children: collapseNonAncestors(nd.children, targetPath) };
        }
        return nd;
      });
    },
    [],
  );

  const doReveal = useCallback(async (targetPath: string) => {
    const fullTarget = targetPath.startsWith(rootPath) ? targetPath : `${rootPath}/${targetPath}`;
    const relative = fullTarget.replace(rootPath + '/', '');
    const segments = relative.split('/');

    const ancestorPaths: string[] = [];
    let dirPath = rootPath;
    for (let i = 0; i < segments.length - 1; i++) {
      dirPath = `${dirPath}/${segments[i]}`;
      ancestorPaths.push(dirPath);
    }

    let localTree = nodes;
    const toLoad: Array<{ path: string; children: TreeNode[] }> = [];

    for (const ap of ancestorPaths) {
      const node = findNode(localTree, ap);
      if (node == null || node.entry.type !== 'dir') break;
      if (node.children == null) {
        const children = await loadChildren(ap);
        toLoad.push({ path: ap, children });
        localTree = updateNode(localTree, ap, (nd) => ({ ...nd, children, expanded: true }));
      }
    }

    setNodes((prev) => {
      let result = collapseNonAncestors(prev, fullTarget);
      for (const { path, children } of toLoad) {
        result = updateNode(result, path, (nd) => ({ ...nd, children, loading: false, expanded: true }));
      }
      for (const ap of ancestorPaths) {
        result = updateNode(result, ap, (nd) => nd.entry.type === 'dir' ? { ...nd, expanded: true } : nd);
      }
      return result;
    });

    setTimeout(() => {
      const el = containerRef.current?.querySelector(`[data-filepath="${fullTarget}"]`);
      el?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    }, 80);
  }, [nodes, rootPath, findNode, loadChildren, updateNode, collapseNonAncestors]);

  useImperativeHandle(ref, () => ({
    revealPath: (targetPath: string) => { void doReveal(targetPath); },
  }), [doReveal]);

  useEffect(() => {
    if (!reportReady) return;
    onExpandedChangeRef.current?.(collectExpanded(nodes));
  }, [nodes, reportReady]);

  const initialExpandDone = useRef(false);
  useEffect(() => {
    if (!loaded || nodes.length === 0 || initialExpandDone.current) return;
    if (initialExpanded === undefined) return;
    initialExpandDone.current = true;

    if (initialExpanded.length > 0) {
      const sorted = [...initialExpanded].sort((a, b) => a.length - b.length);
      void (async () => {
        let localTree = nodes;
        const toLoad: Array<{ path: string; children: TreeNode[] }> = [];
        for (const dirPath of sorted) {
          const node = findNode(localTree, dirPath);
          if (node == null || node.entry.type !== 'dir') continue;
          if (node.children == null) {
            try {
              const children = await loadChildren(dirPath);
              toLoad.push({ path: dirPath, children });
              localTree = updateNode(localTree, dirPath, (nd) => ({ ...nd, children, expanded: true }));
            } catch { /* skip dirs that fail to load */ }
          } else {
            localTree = updateNode(localTree, dirPath, (nd) => ({ ...nd, expanded: true }));
          }
        }
        setNodes((prev) => {
          let result = prev;
          for (const { path, children } of toLoad) {
            result = updateNode(result, path, (nd) => ({ ...nd, children, loading: false, expanded: true }));
          }
          for (const dirPath of sorted) {
            result = updateNode(result, dirPath, (nd) => nd.entry.type === 'dir' ? { ...nd, expanded: true } : nd);
          }
          return result;
        });
        setReportReady(true);
        if (activePath != null) {
          setTimeout(() => {
            const el = containerRef.current?.querySelector(`[data-filepath="${activePath}"]`);
            el?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
          }, 80);
        }
      })();
    } else if (activePath != null) {
      void doReveal(activePath);
      setReportReady(true);
    } else {
      setReportReady(true);
    }
  }, [loaded, nodes.length, initialExpanded]);

  return (
    <div ref={containerRef} className="py-1 overflow-y-auto overflow-x-hidden h-full">
      {nodes.length === 0 && loaded && (
        <span className="text-xs text-subtle-ui px-3 py-2 block">No files</span>
      )}
      {nodes.map((node) => (
        <TreeNodeRow
          key={node.path} node={node} depth={0}
          activePath={activePath} onToggle={handleToggle} onSelect={onFileSelect}
          gitStatus={gitStatus} rootPath={rootPath} isLight={isLight}
        />
      ))}
    </div>
  );
});

export default FileTree;
