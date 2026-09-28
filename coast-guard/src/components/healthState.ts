export type HealthState = 'checking' | 'starting' | 'healthy' | 'unhealthy';

// Service status includes Docker health on both local and remote instances.
export function serviceHealth(status: string): HealthState {
  if (status === 'running (starting)') return 'starting';
  if (status === 'running' || status === 'running (healthy)') return 'healthy';
  return 'unhealthy';
}

export function portHealth(healthy: boolean | undefined, status?: string): HealthState {
  if (status != null) {
    const state = serviceHealth(status);
    if (state !== 'healthy') return state;
  }
  if (healthy === undefined) return 'checking';
  return healthy ? 'healthy' : 'unhealthy';
}

export function isServiceRunning(status: string): boolean {
  return status === 'running' || status.startsWith('running (');
}

export const serviceHealthColors = {
  checking: 'text-subtle-ui',
  starting: 'text-amber-600 dark:text-amber-400',
  healthy: 'text-emerald-600 dark:text-emerald-400',
  unhealthy: 'text-rose-500',
};
