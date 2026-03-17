import { Link, useNavigate } from 'react-router';
import { CaretRight } from '@phosphor-icons/react';
import { useMemo, useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { api } from '../api/endpoints';
import _DOC_ORDER from '../generated/doc-order.json';

const DOC_ORDER: Record<string, number> = _DOC_ORDER;

export interface TreeNode {
  name: string;
  path: string;
  type: 'file' | 'dir';
  children?: TreeNode[];
}

interface DocsSidebarProps {
  tree: TreeNode[];
  activePath: string;
}

const DOC_TITLE_KEYS: Record<string, string> = {
  'README.md': 'docs.nav.overview',
  'GETTING_STARTED.md': 'docs.nav.gettingStarted',
  'SKILLS_FOR_HOST_AGENTS.md': 'docs.nav.skillsForHostAgents',
  'learn-coasts-videos/README.md': 'docs.nav.learnCoasts',
  'learn-coasts-videos/coasts.md': 'docs.nav.learnCoastsCoasts',
  'learn-coasts-videos/ports.md': 'docs.nav.learnCoastsPorts',
  'learn-coasts-videos/assign.md': 'docs.nav.learnCoastsAssign',
  'learn-coasts-videos/checkout.md': 'docs.nav.learnCoastsCheckout',
  'learn-coasts-videos/volumes.md': 'docs.nav.learnCoastsVolumes',
  'learn-coasts-videos/secrets.md': 'docs.nav.learnCoastsSecrets',
  'learn-coasts-videos/getting-started.md': 'docs.nav.learnCoastsGettingStarted',
  'learn-coasts-videos/coast-ui.md': 'docs.nav.learnCoastsCoastUi',
  'concepts_and_terminology/README.md': 'docs.nav.conceptsAndTerminology',
  'concepts_and_terminology/COASTS.md': 'docs.nav.coasts',
  'concepts_and_terminology/PORTS.md': 'docs.nav.ports',
  'concepts_and_terminology/PRIMARY_PORT_AND_DNS.md': 'docs.nav.primaryPortAndDns',
  'concepts_and_terminology/ASSIGN.md': 'docs.nav.assign',
  'concepts_and_terminology/CHECKOUT.md': 'docs.nav.checkout',
  'concepts_and_terminology/LOOKUP.md': 'docs.nav.lookup',
  'concepts_and_terminology/DAEMON.md': 'docs.nav.coastDaemon',
  'concepts_and_terminology/CLI.md': 'docs.nav.coastCli',
  'concepts_and_terminology/COASTGUARD.md': 'docs.nav.coastguard',
  'concepts_and_terminology/VOLUMES.md': 'docs.nav.volumeTopology',
  'concepts_and_terminology/SHARED_SERVICES.md': 'docs.nav.sharedServices',
  'concepts_and_terminology/SECRETS.md': 'docs.nav.secretsAndExtractors',
  'concepts_and_terminology/BUILDS.md': 'docs.nav.builds',
  'concepts_and_terminology/COASTFILE_TYPES.md': 'docs.nav.coastfileTypes',
  'concepts_and_terminology/RUNTIMES_AND_SERVICES.md': 'docs.nav.runtimesAndServices',
  'concepts_and_terminology/LOGS.md': 'docs.nav.logs',
  'concepts_and_terminology/EXEC_AND_DOCKER.md': 'docs.nav.execAndDocker',
  'concepts_and_terminology/AGENT_SHELLS.md': 'docs.nav.agentShells',
  'concepts_and_terminology/MCP_SERVERS.md': 'docs.nav.mcpServers',
  'concepts_and_terminology/FILESYSTEM.md': 'docs.nav.filesystem',
  'concepts_and_terminology/BARE_SERVICES.md': 'docs.nav.bareServices',
  'concepts_and_terminology/MIXED_SERVICE_TYPES.md': 'docs.nav.mixedServiceTypes',
  'concepts_and_terminology/TROUBLESHOOTING.md': 'docs.nav.troubleshooting',
  'coastfiles/README.md': 'docs.nav.coastfiles',
  'coastfiles/PROJECT.md': 'docs.nav.coastfileProject',
  'coastfiles/PORTS.md': 'docs.nav.coastfilePorts',
  'coastfiles/SHARED_SERVICES.md': 'docs.nav.coastfileSharedServices',
  'coastfiles/SERVICES.md': 'docs.nav.coastfileBareServices',
  'coastfiles/SECRETS.md': 'docs.nav.coastfileSecrets',
  'coastfiles/VOLUMES.md': 'docs.nav.coastfileVolumes',
  'coastfiles/ASSIGN.md': 'docs.nav.coastfileAssign',
  'coastfiles/INHERITANCE.md': 'docs.nav.coastfileInheritance',
  'coastfiles/WORKTREE_DIR.md': 'docs.nav.coastfileWorktreeDir',
  'coastfiles/AGENT_SHELL.md': 'docs.nav.coastfileAgentShell',
  'coastfiles/MCP.md': 'docs.nav.coastfileMcp',
  'harnesses/README.md': 'docs.nav.harnesses',
  'harnesses/CODEX.md': 'docs.nav.harnessCodex',
  'harnesses/CONDUCTOR.md': 'docs.nav.harnessConductor',
  'harnesses/T3_CODE.md': 'docs.nav.harnessT3Code',
  'recipes/README.md': 'docs.nav.recipes',
  'recipes/FULLSTACK_MONOREPO.md': 'docs.nav.recipesFullstackMonorepo',
};

function docRoute(node: TreeNode): string {
  if (node.type === 'dir') {
    return '/docs/' + node.path;
  }
  const route = node.path.replace(/\.md$/, '');
  if (route.endsWith('/README') || route === 'README') {
    return '/docs/' + route.replace(/\/?README$/, '');
  }
  return '/docs/' + route;
}

function isActive(node: TreeNode, activePath: string): boolean {
  return docRoute(node) === activePath;
}

function formatName(name: string): string {
  const base = name
    .replace(/\.md$/, '')
    .replace(/^README$/, 'Overview')
    .replace(/[_-]/g, ' ');
  return base.replace(/\b\w/g, (c) => c.toUpperCase());
}

function titleLookupPath(node: TreeNode): string {
  if (node.type === 'dir') return `${node.path}/README.md`;
  return node.path;
}

function displayName(node: TreeNode, t: (key: string) => string): string {
  const key = DOC_TITLE_KEYS[titleLookupPath(node)];
  if (key != null) {
    const translated = t(key);
    if (translated !== key) return translated;
  }
  return formatName(node.name);
}

function sortNodes(nodes: TreeNode[]): TreeNode[] {
  const sorted = [...nodes].sort((a, b) => {
    const orderA = DOC_ORDER[titleLookupPath(a)] ?? Number.MAX_SAFE_INTEGER;
    const orderB = DOC_ORDER[titleLookupPath(b)] ?? Number.MAX_SAFE_INTEGER;

    if (orderA !== orderB) return orderA - orderB;

    if (a.type !== b.type) {
      return a.type === 'file' ? -1 : 1;
    }

    return a.path.localeCompare(b.path);
  });

  return sorted.map((node) => {
    if (node.type === 'dir' && node.children != null) {
      return {
        ...node,
        children: sortNodes(node.children),
      };
    }
    return node;
  });
}

function computeAncestorDirs(tree: TreeNode[], activePath: string): Set<string> {
  const ancestors = new Set<string>();

  function walk(nodes: TreeNode[], _parentPath: string | null): boolean {
    for (const node of nodes) {
      if (node.type === 'dir') {
        const dirRoute = docRoute(node);
        const childMatch =
          dirRoute === activePath ||
          (node.children != null && walk(node.children, node.path));
        if (childMatch) {
          ancestors.add(node.path);
          return true;
        }
      } else if (docRoute(node) === activePath) {
        return true;
      }
    }
    return false;
  }

  walk(tree, null);
  return ancestors;
}

function TreeItem({
  node,
  activePath,
  depth,
  t,
  locale,
  expanded,
  onToggle,
  navigate,
}: {
  node: TreeNode;
  activePath: string;
  depth: number;
  t: (key: string) => string;
  locale: string;
  expanded: Set<string>;
  onToggle: (path: string) => void;
  navigate: (to: string) => void;
}) {
  if (node.type === 'dir') {
    const isExpanded = expanded.has(node.path);

    return (
      <div>
        <button
          type="button"
          onClick={() => {
            if (isExpanded) {
              onToggle(node.path);
            } else {
              onToggle(node.path);
              const route = docRoute(node);
              const alreadyInside = activePath === route || activePath.startsWith(route + '/');
              if (!alreadyInside) {
                navigate(route);
                api.track('docs_sidebar_navigate', {
                  locale,
                  node_type: 'dir',
                  node_path: titleLookupPath(node),
                  route,
                });
              }
            }
          }}
          className="w-full flex items-center gap-0.5 rounded-md text-sm transition-colors text-muted-ui hover:text-main hover:bg-white/10 dark:hover:bg-white/5 cursor-pointer"
          style={{ paddingLeft: `${depth * 0.75}rem` }}
        >
          <span className="flex-shrink-0 p-1">
            <CaretRight
              size={12}
              className={`transition-transform duration-150 ${isExpanded ? 'rotate-90' : ''}`}
            />
          </span>
          <span className="truncate text-left py-1.5 pr-2">{displayName(node, t)}</span>
        </button>
        {isExpanded && node.children != null && (
          <div className="ml-2.5 border-l border-[var(--border)]">
            {node.children.map((child) => (
              <TreeItem
                key={child.path}
                node={child}
                activePath={activePath}
                depth={depth + 1}
                t={t}
                locale={locale}
                expanded={expanded}
                onToggle={onToggle}
                navigate={navigate}
              />
            ))}
          </div>
        )}
      </div>
    );
  }

  const active = isActive(node, activePath);

  return (
    <Link
      to={docRoute(node)}
      onClick={() => {
        api.track('docs_sidebar_navigate', {
          locale,
          node_type: 'file',
          node_path: node.path,
          route: docRoute(node),
        });
      }}
      style={{ paddingLeft: `${depth * 0.75}rem` }}
      className={`flex items-center gap-0.5 rounded-md text-sm transition-colors ${
        active
          ? 'text-main font-semibold bg-white/20 dark:bg-white/10'
          : 'text-muted-ui hover:text-main hover:bg-white/10 dark:hover:bg-white/5'
      }`}
    >
      <span className="flex-shrink-0 p-1 invisible" aria-hidden="true">
        <CaretRight size={12} />
      </span>
      <span className="truncate py-1.5 pr-2">{displayName(node, t)}</span>
    </Link>
  );
}

export default function DocsSidebar({ tree, activePath }: DocsSidebarProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const navigate = useNavigate();
  const sortedTree = useMemo(() => sortNodes(tree), [tree]);

  const [expanded, setExpanded] = useState<Set<string>>(() =>
    computeAncestorDirs(sortedTree, activePath),
  );

  useEffect(() => {
    const ancestors = computeAncestorDirs(sortedTree, activePath);
    setExpanded((prev) => {
      let changed = false;
      const next = new Set(prev);
      for (const a of ancestors) {
        if (!next.has(a)) {
          next.add(a);
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [activePath, sortedTree]);

  const handleToggle = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  return (
    <nav className="flex flex-col gap-0.5 py-2 px-1">
      {sortedTree.map((node) => (
        <TreeItem
          key={node.path}
          node={node}
          activePath={activePath}
          depth={0}
          t={t}
          locale={locale}
          expanded={expanded}
          onToggle={handleToggle}
          navigate={navigate}
        />
      ))}
    </nav>
  );
}
