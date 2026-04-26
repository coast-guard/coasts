/**
 * Helpers that build absolute `ws://` / `wss://` URLs for the
 * SSG's WebSocket endpoints. Centralized so the tab components
 * don't all reimplement protocol/host derivation.
 */

function wsBase(): string {
    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    return `${proto}//${window.location.host}`;
}

/** PTY into the project's SSG outer DinD shell. Backs the Exec tab. */
export function ssgTerminalWsUrl(project: string): string {
    return `${wsBase()}/api/v1/ssg/terminal?project=${encodeURIComponent(project)}`;
}

/**
 * Sentinel `service` values understood by the SSG logs WebSocket.
 *
 * Mirror the `service` query-param contract on the daemon side
 * (`ws_ssg_logs.rs`): the SPA never passes a literal "all
 * services" UI label as the WS arg; it sends one of these
 * sentinels so the daemon picks the right command.
 */
export const SSG_LOGS_SERVICE_ALL = '__all__';
export const SSG_LOGS_SERVICE_OUTER = '__outer__';

/**
 * Streaming logs for the project's SSG.
 *
 * - `service` omitted or `"__all__"` (default): streams
 *   `docker compose logs --follow` for every inner service. Lines
 *   are prefixed with `<service>-1  | ` so the SPA can render
 *   service color tags via the same `parseLine` helper used by
 *   the regular instance logs view.
 * - `service="__outer__"`: streams the outer DinD container's
 *   stdout/stderr (containerd boot, image pulls, …).
 * - `service="<name>"`: streams `docker compose logs --follow <name>`
 *   inside the DinD.
 */
export function ssgLogsWsUrl(
    project: string,
    service?: string,
    tail?: number,
): string {
    const params = new URLSearchParams({ project });
    if (service != null) {
        params.set('service', service);
    }
    if (tail != null) {
        params.set('tail', String(tail));
    }
    return `${wsBase()}/api/v1/ssg/logs/stream?${params.toString()}`;
}

/** Streaming docker-stats samples for the SSG outer DinD container. */
export function ssgStatsWsUrl(project: string): string {
    return `${wsBase()}/api/v1/ssg/stats/stream?project=${encodeURIComponent(project)}`;
}
