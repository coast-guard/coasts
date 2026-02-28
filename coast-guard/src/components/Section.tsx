export default function Section({ title, children }: { readonly title: string; readonly children: React.ReactNode }) {
  return (
    <div className="glass-panel p-4 mb-4">
      <h3 className="text-xs font-semibold text-subtle-ui uppercase tracking-wider mb-3">{title}</h3>
      {children}
    </div>
  );
}
