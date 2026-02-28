import { useMemo } from 'react';
import { buildHostServiceExecTerminalConfig } from '../hooks/useTerminalSessions';
import PersistentTerminal from '../components/PersistentTerminal';

interface Props {
  readonly project: string;
  readonly service: string;
}

export default function HostServiceExecTab({ project, service }: Props) {
  const config = useMemo(
    () => buildHostServiceExecTerminalConfig(project, service),
    [project, service],
  );

  return <PersistentTerminal config={config} />;
}
