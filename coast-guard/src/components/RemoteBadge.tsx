import { Cloud } from '@phosphor-icons/react';

interface Props {
  readonly remoteName: string;
}

/**
 * Badge indicating that an instance is running on a remote VM.
 */
export default function RemoteBadge({ remoteName }: Props) {
  return (
    <span
      className="inline-flex items-center gap-1 px-2 py-0.5 text-[11px] font-medium rounded-full bg-indigo-500/12 border border-indigo-500/30 text-indigo-700 dark:text-indigo-300"
      title={`Running on remote: ${remoteName}`}
    >
      <Cloud size={12} weight="fill" />
      {remoteName}
    </span>
  );
}
