import { useQuery, useMutation } from '@tanstack/react-query';
import { api } from '../api/endpoints';

export function useDockerInfo() {
  return useQuery({
    queryKey: ['dockerInfo'],
    queryFn: () => api.dockerInfo(),
    refetchInterval: 60_000,
    retry: false,
  });
}

export function useOpenDockerSettingsMutation() {
  return useMutation({
    mutationFn: () => api.openDockerSettings(),
  });
}
