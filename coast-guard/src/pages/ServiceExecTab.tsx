import { useMemo } from 'react';
import type { ProjectName, InstanceName } from '../types/branded';
import { buildServiceExecTerminalConfig } from '../hooks/useTerminalSessions';
import PersistentTerminal from '../components/PersistentTerminal';

interface Props {
  readonly project: ProjectName;
  readonly name: InstanceName;
  readonly service: string;
}

export default function ServiceExecTab({ project, name, service }: Props) {
  const config = useMemo(
    () => buildServiceExecTerminalConfig(project, name, service),
    [project, name, service],
  );

  return <PersistentTerminal config={config} />;
}
