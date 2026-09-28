import { usePortHealth } from '../api/hooks';
import HealthDot from './HealthDot';
import { portHealth } from './healthState';

interface Props {
  project: string;
  name: string;
  service?: string | null | undefined;
  size?: number;
}

export default function PrimaryPortHealthDot({ project, name, service, size = 6 }: Props) {
  const { data } = usePortHealth(project, name);
  const svc = service ?? 'web';
  const port = data?.ports?.find((p) => p.logical_name === svc);
  return <HealthDot state={portHealth(port?.healthy, port?.service_status)} size={size} />;
}
