export default function KeyValue({ label, value }: { readonly label: string; readonly value: string }) {
  if (value.length === 0) return null;
  return (
    <div className="flex gap-3 py-1.5 border-b border-[var(--border)] last:border-0">
      <span className="text-xs text-subtle-ui w-36 shrink-0 font-medium">{label}</span>
      <span className="text-xs font-mono text-main break-all">{value}</span>
    </div>
  );
}
