import { useMemo } from 'react';
import PersistentTerminal from '../PersistentTerminal';
import { buildSsgTerminalConfig } from '../../hooks/useTerminalSessions';

interface SsgExecTabProps {
    readonly project: string;
}

/**
 * SSG → Exec tab. Reuses the same `PersistentTerminal` chrome
 * (Shell N tabs, theme picker, fullscreen, recording) as every
 * other terminal in the app, pointed at
 * `/api/v1/ssg/terminal?project=<p>`. Session persistence is a
 * no-op for SSGs today — see `buildSsgTerminalConfig`'s docs.
 */
export default function SsgExecTab({ project }: SsgExecTabProps) {
    const config = useMemo(() => buildSsgTerminalConfig(project), [project]);
    return <PersistentTerminal config={config} />;
}
