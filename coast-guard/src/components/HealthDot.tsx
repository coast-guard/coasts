import type { HealthState } from './healthState';

interface Props {
  healthy?: boolean | undefined;
  state?: HealthState;
  size?: number;
  title?: string;
}

const appearances = {
  checking: { color: 'bg-slate-400/50', title: 'Checking...' },
  starting: { color: 'bg-amber-500 animate-pulse', title: 'Service is starting' },
  healthy: { color: 'bg-emerald-500', title: 'Port is up' },
  unhealthy: { color: 'bg-rose-500', title: 'Port or service is down' },
};

export default function HealthDot({ healthy, state, size = 6, title }: Props) {
  const resolved = state ?? (healthy === undefined ? 'checking' : healthy ? 'healthy' : 'unhealthy');
  const appearance = appearances[resolved];
  return (
    <span
      className={`inline-block rounded-full ${appearance.color}`}
      style={{ width: size, height: size }}
      title={title ?? appearance.title}
    />
  );
}
