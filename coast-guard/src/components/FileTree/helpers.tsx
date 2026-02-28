import { FileTs, FileJs, FileHtml, FileCss, FileRs, GearSix, File } from '@phosphor-icons/react';

export interface FileEntry {
  readonly name: string;
  readonly type: string;
  readonly size: number;
}

export interface TreeNode {
  entry: FileEntry;
  path: string;
  children: TreeNode[] | null;
  loading: boolean;
  expanded: boolean;
}

export function getFileIcon(name: string) {
  const ext = name.split('.').pop()?.toLowerCase() ?? '';
  switch (ext) {
    case 'ts': case 'tsx':
      return <FileTs size={16} className="text-blue-400 shrink-0" />;
    case 'js': case 'jsx': case 'mjs': case 'cjs':
      return <FileJs size={16} className="text-amber-400 shrink-0" />;
    case 'html': case 'htm':
      return <FileHtml size={16} className="text-orange-400 shrink-0" />;
    case 'css': case 'scss': case 'less':
      return <FileCss size={16} className="text-blue-300 shrink-0" />;
    case 'rs':
      return <FileRs size={16} className="text-orange-500 shrink-0" />;
    case 'json': case 'toml': case 'yaml': case 'yml':
      return <GearSix size={16} className="text-slate-400 shrink-0" />;
    default:
      return <File size={16} className="text-slate-400 shrink-0" />;
  }
}

export function gitStatusColor(status: string | undefined): string {
  if (status == null) return '';
  if (status === '??' || status === 'A') return 'text-emerald-400';
  if (status === 'M' || status === 'MM' || status === 'AM') return 'text-amber-400';
  if (status === 'D') return 'text-rose-400 line-through';
  if (status === 'R') return 'text-blue-400';
  return 'text-amber-400';
}

export function dirHasChanges(dirPath: string, rootPath: string, gitStatus: Map<string, string> | undefined): boolean {
  if (gitStatus == null || gitStatus.size === 0) return false;
  const prefix = dirPath.replace(rootPath + '/', '') + '/';
  for (const [path] of gitStatus) {
    if (path.startsWith(prefix)) return true;
  }
  return false;
}

export function collectExpanded(nodes: TreeNode[]): string[] {
  const result: string[] = [];
  for (const nd of nodes) {
    if (nd.entry.type === 'dir' && nd.expanded) {
      result.push(nd.path);
      if (nd.children != null) result.push(...collectExpanded(nd.children));
    }
  }
  return result;
}
