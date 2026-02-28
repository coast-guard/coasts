export default function DotLoader({ className }: { readonly className?: string }) {
  return (
    <span className={`inline-flex gap-0.5 ${className ?? ''}`}>
      <span className="h-1 w-1 rounded-full bg-current animate-[dotPulse_1.4s_ease-in-out_0s_infinite]" />
      <span className="h-1 w-1 rounded-full bg-current animate-[dotPulse_1.4s_ease-in-out_0.2s_infinite]" />
      <span className="h-1 w-1 rounded-full bg-current animate-[dotPulse_1.4s_ease-in-out_0.4s_infinite]" />
    </span>
  );
}
