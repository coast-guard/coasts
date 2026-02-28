import { SpinnerGap, CheckCircle, XCircle, WarningCircle } from '@phosphor-icons/react';

export default function StepIcon({ status }: { readonly status: string }) {
  if (status === 'ok') return <CheckCircle size={14} weight="fill" className="text-green-500 shrink-0" />;
  if (status === 'fail') return <XCircle size={14} weight="fill" className="text-rose-500 shrink-0" />;
  if (status === 'warn') return <WarningCircle size={14} weight="fill" className="text-amber-500 shrink-0" />;
  if (status === 'started') return <SpinnerGap size={14} className="animate-spin text-blue-400 shrink-0" />;
  if (status === 'skip') return <span className="w-3.5 h-3.5 shrink-0 text-center text-[10px] text-subtle-ui">—</span>;
  return <span className="w-3.5 h-3.5 shrink-0" />;
}
