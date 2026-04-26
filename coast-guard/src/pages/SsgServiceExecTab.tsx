import { useMemo } from 'react';
import { buildSsgServiceExecTerminalConfig } from '../hooks/useTerminalSessions';
import PersistentTerminal from '../components/PersistentTerminal';

interface Props {
    readonly project: string;
    readonly service: string;
}

/**
 * SSG flavor of {@link ServiceExecTab}: spawns a `docker exec -it
 * <ssg_outer> docker exec -it <inner_container> sh` PTY into the
 * named SSG inner service. Reuses the shared `PersistentTerminal`
 * component so the user gets the same terminal UX (xterm + fit
 * addon + scrollback persistence + session reconnect) as the
 * per-instance equivalent.
 */
export default function SsgServiceExecTab({ project, service }: Props) {
    const config = useMemo(
        () => buildSsgServiceExecTerminalConfig(project, service),
        [project, service],
    );

    return <PersistentTerminal config={config} />;
}
