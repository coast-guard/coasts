export default function EmptyState({ message }: { readonly message: string }) {
  return (
    <div className="flex flex-col items-center justify-center py-12 text-subtle-ui">
      <p className="text-sm">{message}</p>
    </div>
  );
}
