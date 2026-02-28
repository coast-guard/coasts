import type { McpServerSummary } from '../types/api';
import McpTypeBadge from './McpTypeBadge';
import McpStatusBadge from './McpStatusBadge';

export default function McpServerRow({
  server,
  selected,
  onSelect,
}: {
  readonly server: McpServerSummary;
  readonly selected: boolean;
  readonly onSelect: () => void;
}) {
  const cmd = server.command ?? '-';
  const cmdWithArgs = server.args.length > 0 ? `${cmd} ${server.args.join(' ')}` : cmd;
  const cmdDisplay = cmdWithArgs.length > 50 ? `${cmdWithArgs.slice(0, 47)}...` : cmdWithArgs;

  return (
    <tr
      className={`border-b border-[var(--border)] last:border-0 cursor-pointer transition-colors ${
        selected ? 'bg-blue-500/10' : 'hover:bg-white/5'
      }`}
      onClick={onSelect}
    >
      <td className="py-2 px-2 text-xs font-mono text-main">{server.name}</td>
      <td className="py-2 px-2"><McpTypeBadge proxy={server.proxy} /></td>
      <td className="py-2 px-2 text-xs font-mono text-main" title={cmdWithArgs}>{cmdDisplay}</td>
      <td className="py-2 px-2"><McpStatusBadge status={server.status} /></td>
    </tr>
  );
}
