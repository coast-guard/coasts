import {
  useQuery,
  useMutation,
  useQueryClient,
} from '@tanstack/react-query';
import type { ProjectName, InstanceName } from '../types/branded';
import { api } from './endpoints';

export const qk = {
  instances: (project?: ProjectName) =>
    project != null ? (['instances', project] as const) : (['instances'] as const),
  projectGit: (project: ProjectName) => ['projectGit', project] as const,
  ports: (project: ProjectName, name: InstanceName) =>
    ['ports', project, name] as const,
  services: (project: ProjectName, name: InstanceName) =>
    ['services', project, name] as const,
  images: (project: ProjectName, name: InstanceName) =>
    ['images', project, name] as const,
  secrets: (project: ProjectName, name: InstanceName) =>
    ['secrets', project, name] as const,
  imageInspect: (project: ProjectName, name: InstanceName, image: string) =>
    ['imageInspect', project, name, image] as const,
  volumes: (project: ProjectName, name: InstanceName) =>
    ['volumes', project, name] as const,
  execSessions: (project: ProjectName, name: InstanceName) =>
    ['execSessions', project, name] as const,
  volumeInspect: (project: ProjectName, name: InstanceName, volume: string) =>
    ['volumeInspect', project, name, volume] as const,
  serviceInspect: (project: string, name: string, service: string) =>
    ['serviceInspect', project, name, service] as const,
  sharedServices: (project: string) =>
    ['sharedServices', project] as const,
  sharedServicesAll: () => ['sharedServicesAll'] as const,
  fileTree: (project: string, name: string, path: string) =>
    ['fileTree', project, name, path] as const,
  fileRead: (project: string, name: string, path: string) =>
    ['fileRead', project, name, path] as const,
  hostServiceInspect: (project: string, service: string) =>
    ['hostServiceInspect', project, service] as const,
  hostImageInspect: (project: string, image: string) =>
    ['hostImageInspect', project, image] as const,
  buildsLs: (project?: string) =>
    ['buildsLs', project] as const,
  buildsInspect: (project: string, buildId?: string) =>
    ['buildsInspect', project, buildId] as const,
  buildsImages: (project: string, buildId?: string) =>
    ['buildsImages', project, buildId] as const,
  buildsDockerImages: (project: string, buildId?: string) =>
    ['buildsDockerImages', project, buildId] as const,
  buildsCompose: (project: string, buildId?: string) =>
    ['buildsCompose', project, buildId] as const,
  buildsCoastfile: (project: string, buildId?: string) =>
    ['buildsCoastfile', project, buildId] as const,
  mcpServers: (project: string, name: string) =>
    ['mcpServers', project, name] as const,
  mcpTools: (project: string, name: string, server: string, tool?: string) =>
    ['mcpTools', project, name, server, tool] as const,
  mcpLocations: (project: string, name: string) =>
    ['mcpLocations', project, name] as const,
} as const;

export function useInstances(project?: ProjectName) {
  return useQuery({
    queryKey: qk.instances(project),
    queryFn: () => api.ls(project),
    refetchInterval: 30_000,
    staleTime: 10_000,
  });
}

export function useProjectGit(project: ProjectName) {
  return useQuery({
    queryKey: qk.projectGit(project),
    queryFn: () => api.projectGit(project),
    refetchInterval: 30_000,
  });
}

export function usePorts(project: ProjectName, name: InstanceName) {
  return useQuery({
    queryKey: qk.ports(project, name),
    queryFn: () => api.ports(name, project),
  });
}

export function useServices(project: ProjectName, name: InstanceName) {
  return useQuery({
    queryKey: qk.services(project, name),
    queryFn: () => api.ps(name, project),
  });
}

export function useImages(project: ProjectName, name: InstanceName) {
  return useQuery({
    queryKey: qk.images(project, name),
    queryFn: () => api.listImages(project, name),
  });
}

export function useSecrets(project: ProjectName, name: InstanceName) {
  return useQuery({
    queryKey: qk.secrets(project, name),
    queryFn: () => api.listSecrets(project, name),
  });
}

export function useImageInspect(project: ProjectName, name: InstanceName, image: string) {
  return useQuery({
    queryKey: qk.imageInspect(project, name, image),
    queryFn: () => api.inspectImage(project, name, image),
    enabled: image.length > 0,
  });
}

export function useVolumes(project: ProjectName, name: InstanceName) {
  return useQuery({
    queryKey: qk.volumes(project, name),
    queryFn: () => api.listVolumes(project, name),
  });
}

export function useExecSessions(project: ProjectName, name: InstanceName, enabled = true) {
  return useQuery({
    queryKey: qk.execSessions(project, name),
    queryFn: () => api.listExecSessions(project, name),
    enabled,
  });
}

export function useVolumeInspect(project: ProjectName, name: InstanceName, volume: string) {
  return useQuery({
    queryKey: qk.volumeInspect(project, name, volume),
    queryFn: () => api.inspectVolume(project, name, volume),
    enabled: volume.length > 0,
  });
}

export function useServiceInspect(project: string, name: string, service: string) {
  return useQuery({
    queryKey: qk.serviceInspect(project, name, service),
    queryFn: () => api.serviceInspect(project, name, service),
    enabled: service.length > 0,
  });
}

interface MutationVars {
  readonly name: InstanceName;
  readonly project: ProjectName;
}

export function useStopMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ name, project }: MutationVars) => api.stop(name, project),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['instances'] }),
  });
}

export function useStartMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ name, project }: MutationVars) => api.start(name, project),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['instances'] }),
  });
}

export function useRmMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ name, project }: MutationVars) => api.rm(name, project),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['instances'] }),
  });
}

export function useRmBuildMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project }: { readonly project: string }) => api.rmBuild(project),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['instances'] });
      void qc.invalidateQueries({ queryKey: ['sharedServicesAll'] });
    },
  });
}

export function useArchiveMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project }: { readonly project: string }) => api.archiveProject(project),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['instances'] });
      void qc.invalidateQueries({ queryKey: ['sharedServicesAll'] });
    },
  });
}

export function useUnarchiveMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project }: { readonly project: string }) => api.unarchiveProject(project),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['instances'] });
      void qc.invalidateQueries({ queryKey: ['sharedServicesAll'] });
    },
  });
}

export function useCheckoutMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project, name }: { readonly project: ProjectName; readonly name?: InstanceName | undefined }) =>
      api.checkout(project, name),
    onSuccess: (_data: unknown, vars: { readonly project: ProjectName; readonly name?: InstanceName | undefined }) => {
      void qc.invalidateQueries({ queryKey: ['instances'] });
      void qc.invalidateQueries({ queryKey: ['ports', vars.project] });
    },
  });
}

interface ServiceMutationVars {
  readonly project: string;
  readonly name: string;
  readonly service: string;
}

export function useFileTree(project: string, name: string, path: string) {
  return useQuery({
    queryKey: qk.fileTree(project, name, path),
    queryFn: () => api.fileTree(project, name, path),
    enabled: path.length > 0,
    staleTime: 30_000,
  });
}

export function useFileRead(project: string, name: string, path: string) {
  return useQuery({
    queryKey: qk.fileRead(project, name, path),
    queryFn: () => api.fileRead(project, name, path),
    enabled: path.length > 0,
    staleTime: 60_000,
  });
}

export function useHostServiceInspect(project: string, service: string) {
  return useQuery({
    queryKey: qk.hostServiceInspect(project, service),
    queryFn: () => api.hostServiceInspect(project, service),
    enabled: service.length > 0,
  });
}

export function useHostImageInspect(project: string, image: string) {
  return useQuery({
    queryKey: qk.hostImageInspect(project, image),
    queryFn: () => api.hostImageInspect(project, image),
    enabled: image.length > 0,
  });
}

export function useSharedServices(project: string) {
  return useQuery({
    queryKey: qk.sharedServices(project),
    queryFn: () => api.sharedLs(project),
    refetchInterval: 10_000,
    staleTime: 5_000,
    enabled: project.length > 0,
  });
}

export function useAllSharedServices() {
  return useQuery({
    queryKey: qk.sharedServicesAll(),
    queryFn: () => api.sharedLsAll(),
    refetchInterval: 30_000,
  });
}


interface SharedServiceMutationVars {
  readonly project: string;
  readonly service: string;
}

export function useSharedStopMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project, service }: SharedServiceMutationVars) =>
      api.sharedStop(project, service),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['sharedServices'] }),
  });
}

export function useSharedStartMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project, service }: SharedServiceMutationVars) =>
      api.sharedStart(project, service),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['sharedServices'] }),
  });
}

export function useSharedRestartMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project, service }: SharedServiceMutationVars) =>
      api.sharedRestart(project, service),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['sharedServices'] }),
  });
}

export function useSharedRmMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project, service }: SharedServiceMutationVars) =>
      api.sharedRm(project, service),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['sharedServices'] }),
  });
}

export function useServiceStopMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project, name, service }: ServiceMutationVars) =>
      api.serviceStop(project, name, service),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['services'] }),
  });
}

export function useServiceStartMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project, name, service }: ServiceMutationVars) =>
      api.serviceStart(project, name, service),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['services'] }),
  });
}

export function useServiceRestartMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project, name, service }: ServiceMutationVars) =>
      api.serviceRestart(project, name, service),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['services'] }),
  });
}

export function useServiceRmMutation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ project, name, service }: ServiceMutationVars) =>
      api.serviceRm(project, name, service),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['services'] }),
  });
}

// --- Builds ---

export function useBuildsLs(project?: string) {
  return useQuery({
    queryKey: qk.buildsLs(project),
    queryFn: () => api.buildsLs(project),
  });
}

export function useBuildsInspect(project: string, buildId?: string) {
  return useQuery({
    queryKey: qk.buildsInspect(project, buildId),
    queryFn: () => api.buildsInspect(project, buildId),
    enabled: project.length > 0,
  });
}

export function useBuildsImages(project: string, buildId?: string) {
  return useQuery({
    queryKey: qk.buildsImages(project, buildId),
    queryFn: () => api.buildsImages(project, buildId),
    enabled: project.length > 0,
  });
}

export function useBuildsDockerImages(project: string, buildId?: string) {
  return useQuery({
    queryKey: qk.buildsDockerImages(project, buildId),
    queryFn: () => api.buildsDockerImages(project, buildId),
    enabled: project.length > 0,
  });
}

export function useBuildsCompose(project: string, buildId?: string) {
  return useQuery({
    queryKey: qk.buildsCompose(project, buildId),
    queryFn: () => api.buildsCompose(project, buildId),
    enabled: project.length > 0,
  });
}

export function useBuildsCoastfile(project: string, buildId?: string) {
  return useQuery({
    queryKey: qk.buildsCoastfile(project, buildId),
    queryFn: () => api.buildsCoastfile(project, buildId),
    enabled: project.length > 0,
  });
}

export function useMcpServers(project: string, name: string) {
  return useQuery({
    queryKey: qk.mcpServers(project, name),
    queryFn: () => api.mcpLs(project, name),
    enabled: project.length > 0 && name.length > 0,
  });
}

export function useMcpTools(project: string, name: string, server: string, tool?: string) {
  return useQuery({
    queryKey: qk.mcpTools(project, name, server, tool),
    queryFn: () => api.mcpTools(project, name, server, tool),
    enabled: project.length > 0 && name.length > 0 && server.length > 0,
  });
}

export function useMcpLocations(project: string, name: string) {
  return useQuery({
    queryKey: qk.mcpLocations(project, name),
    queryFn: () => api.mcpLocations(project, name),
    enabled: project.length > 0 && name.length > 0,
  });
}
