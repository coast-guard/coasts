import { memo } from 'react';
import { CaretRight, CaretDown, Folder, FolderOpen, Circle } from '@phosphor-icons/react';
import { type TreeNode, getFileIcon, gitStatusColor, dirHasChanges } from './helpers';

const TreeNodeRow = memo(function TreeNodeRow({
  node, depth, activePath, onToggle, onSelect, gitStatus, rootPath, isLight,
}: {
  readonly node: TreeNode;
  readonly depth: number;
  readonly activePath: string | null;
  readonly onToggle: (path: string) => void;
  readonly onSelect: (path: string) => void;
  readonly gitStatus: Map<string, string> | undefined;
  readonly rootPath: string;
  readonly isLight: boolean;
}) {
  const isDir = node.entry.type === 'dir';
  const isActive = node.path === activePath;
  const relativePath = node.path.replace(rootPath + '/', '');
  const status = gitStatus?.get(relativePath);
  const colorClass = isDir ? '' : gitStatusColor(status);
  const hasChanges = isDir && dirHasChanges(node.path, rootPath, gitStatus);
  const hoverBg = isLight ? 'hover:bg-black/5' : 'hover:bg-white/5';
  const defaultText = isLight ? 'text-gray-800' : 'text-main';

  return (
    <>
      <div
        data-filepath={node.path}
        className={`flex items-center gap-1 py-0.5 px-2 cursor-pointer text-xs select-none ${hoverBg} rounded ${
          isActive ? 'bg-blue-500/15 text-[var(--primary)]' : colorClass || defaultText
        }`}
        style={{ paddingLeft: `${depth * 16 + 8}px` }}
        onClick={() => { if (isDir) onToggle(node.path); else onSelect(node.path); }}
      >
        {isDir ? (
          <>
            {node.expanded
              ? <CaretDown size={12} className="text-subtle-ui shrink-0" />
              : <CaretRight size={12} className="text-subtle-ui shrink-0" />}
            {node.expanded
              ? <FolderOpen size={16} className="text-amber-500 shrink-0" />
              : <Folder size={16} className="text-amber-500 shrink-0" />}
          </>
        ) : (
          <>
            <span className="w-3 shrink-0" />
            {getFileIcon(node.entry.name)}
          </>
        )}
        <span className="truncate">{node.entry.name}</span>
        {isDir && hasChanges && (
          <Circle size={6} weight="fill" className="text-amber-500/60 shrink-0 ml-auto" />
        )}
        {node.loading && (
          <span className="ml-auto text-[10px] text-subtle-ui animate-pulse">...</span>
        )}
      </div>
      {isDir && node.expanded && node.children != null && (
        node.children.map((child) => (
          <TreeNodeRow
            key={child.path} node={child} depth={depth + 1}
            activePath={activePath} onToggle={onToggle} onSelect={onSelect}
            gitStatus={gitStatus} rootPath={rootPath} isLight={isLight}
          />
        ))
      )}
    </>
  );
});

export default TreeNodeRow;
