import type { ProjectName, InstanceName } from '../types/branded';
import type {
  UpdateCheckResponse,
  UpdateApplyResponse,
  LsResponse,
  StopResponse,
  StartResponse,
  RmResponse,
  RmBuildResponse,
  ArchiveProjectResponse,
  UnarchiveProjectResponse,
  CheckoutResponse,
  PortsResponse,
  PsResponse,
  LogsResponse,
  NameProjectRequest,
  LogsRequest,
  CheckoutRequest,
  ClearLogsResponse,
  SessionInfo,
  ExecSessionInfo,
  AgentShellAvailableResponse,
  SpawnAgentShellResponse,
  ActivateAgentShellResponse,
  CloseAgentShellResponse,
  AgentShellActionRequest,
  SpawnAgentShellRequest,
  ProjectGitResponse,
  GetSettingResponse,
  SetSettingBody,
  SettingResponse,
  ImageSummary,
  ImageInspectResponse,
  SecretInfo,
  RevealSecretResponse,
  RemoteResponse,
  SshKeysResponse,
  SshKeyValidateResponse,
  RemoteStatsResponse,
  RerunExtractorsResponse,
  RestartServicesResponse,
  VolumeSummaryResponse,
  VolumeInspectResponse,
  ServiceInspectResponse,
  ServiceControlRequest,
  SuccessResponse,
  PortHealthStatus,
  SharedResponse,
  SharedAllResponse,
  BuildSummary,
  BuildsInspectResponse,
  BuildsImagesResponse,
  BuildsDockerImagesResponse,
  BuildsContentResponse,
  BuildProgressEvent,
  CoastfileTypesResponse,
  DockerInfoResponse,
  OpenDockerSettingsResponse,
  FileEntry,
  FileReadResponse,
  FilesWriteBody,
  GitFileStatus,
  GrepMatch,
  McpLsResponse,
  McpToolsResponse,
  McpLocationsResponse,
} from '../types/api';
import { get, post, del, beacon } from './client';
import { consumeSSE } from './sse';

type AnalyticsMetadata = Record<string, string>;

export interface DocsSearchResult {
  path: string;
  route: string;
  heading: string;
  snippet: string;
  score: number;
}

export interface DocsSearchResponse {
  query: string;
  locale: string;
  strategy: string;
  results: DocsSearchResult[];
}

export const api = {
  ls(project?: ProjectName): Promise<LsResponse> {
    const q = project != null ? `?project=${encodeURIComponent(project)}` : '';
    return get<LsResponse>(`/ls${q}`);
  },

  projectGit(project: ProjectName): Promise<ProjectGitResponse> {
    return get<ProjectGitResponse>(`/project/git?project=${encodeURIComponent(project)}`);
  },

  stop(name: InstanceName, project: ProjectName): Promise<StopResponse> {
    return post<NameProjectRequest, StopResponse>('/stop', { name, project });
  },

  start(name: InstanceName, project: ProjectName): Promise<StartResponse> {
    return post<NameProjectRequest, StartResponse>('/start', { name, project });
  },

  restartServices(name: InstanceName, project: ProjectName): Promise<RestartServicesResponse> {
    return post<NameProjectRequest, RestartServicesResponse>('/restart-services', { name, project });
  },

  rm(name: InstanceName, project: ProjectName): Promise<RmResponse> {
    return post<NameProjectRequest, RmResponse>('/rm', { name, project });
  },

  rmBuild(project: string, buildIds?: string[]): Promise<{ complete?: RmBuildResponse; error?: { error: string } }> {
    return consumeSSE<never, RmBuildResponse>('/api/v1/stream/rm-build', { project, build_ids: buildIds ?? [] });
  },

  archiveProject(project: string): Promise<ArchiveProjectResponse> {
    return post<{ project: string }, ArchiveProjectResponse>('/archive', { project });
  },

  unarchiveProject(project: string): Promise<UnarchiveProjectResponse> {
    return post<{ project: string }, UnarchiveProjectResponse>('/unarchive', { project });
  },

  checkout(project: ProjectName, name?: InstanceName): Promise<CheckoutResponse> {
    return post<CheckoutRequest, CheckoutResponse>('/checkout', { name, project });
  },

  ports(name: InstanceName, project: ProjectName): Promise<PortsResponse> {
    return post<{ action: string; name: InstanceName; project: ProjectName }, PortsResponse>(
      '/ports',
      { action: 'List', name, project },
    );
  },

  setPrimaryPort(
    name: InstanceName,
    project: ProjectName,
    service: string,
  ): Promise<PortsResponse> {
    return post<
      { action: string; name: InstanceName; project: ProjectName; service: string },
      PortsResponse
    >('/ports', { action: 'SetPrimary', name, project, service });
  },

  unsetPrimaryPort(name: InstanceName, project: ProjectName): Promise<PortsResponse> {
    return post<{ action: string; name: InstanceName; project: ProjectName }, PortsResponse>(
      '/ports',
      { action: 'UnsetPrimary', name, project },
    );
  },

  ps(name: InstanceName, project: ProjectName): Promise<PsResponse> {
    return post<NameProjectRequest, PsResponse>('/ps', { name, project });
  },

  logs(
    name: InstanceName,
    project: ProjectName,
    service?: string,
  ): Promise<LogsResponse> {
    return post<LogsRequest, LogsResponse>('/logs', {
      name,
      project,
      service: service ?? null,
      follow: false,
    });
  },

  clearLogs(name: InstanceName, project: ProjectName): Promise<ClearLogsResponse> {
    return post<NameProjectRequest, ClearLogsResponse>('/logs/clear', { name, project });
  },

  listHostSessions(project: ProjectName): Promise<readonly SessionInfo[]> {
    return get<readonly SessionInfo[]>(
      `/host/sessions?project=${encodeURIComponent(project)}`,
    );
  },

  deleteHostSession(id: string): Promise<void> {
    return del(`/host/sessions?id=${encodeURIComponent(id)}`);
  },

  listExecSessions(
    project: ProjectName,
    name: InstanceName,
  ): Promise<readonly ExecSessionInfo[]> {
    return get<readonly ExecSessionInfo[]>(
      `/exec/sessions?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}`,
    );
  },

  agentShellAvailable(
    project: string,
    name: string,
  ): Promise<AgentShellAvailableResponse> {
    return get<AgentShellAvailableResponse>(
      `/exec/agent-shell?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}`,
    );
  },

  spawnAgentShell(
    project: string,
    name: string,
  ): Promise<SpawnAgentShellResponse> {
    return post<SpawnAgentShellRequest, SpawnAgentShellResponse>(
      '/exec/agent-shell/spawn',
      { project, name },
    );
  },

  activateAgentShell(
    project: string,
    name: string,
    shellId: number,
  ): Promise<ActivateAgentShellResponse> {
    return post<AgentShellActionRequest, ActivateAgentShellResponse>(
      '/exec/agent-shell/activate',
      { project, name, shell_id: shellId },
    );
  },

  closeAgentShell(
    project: string,
    name: string,
    shellId: number,
  ): Promise<CloseAgentShellResponse> {
    return post<AgentShellActionRequest, CloseAgentShellResponse>(
      '/exec/agent-shell/close',
      { project, name, shell_id: shellId },
    );
  },

  deleteExecSession(id: string): Promise<void> {
    return del(`/exec/sessions?id=${encodeURIComponent(id)}`);
  },

  async getSetting(key: string): Promise<string | null> {
    try {
      const res = await get<GetSettingResponse>(`/settings?key=${encodeURIComponent(key)}`);
      return res.value ?? null;
    } catch {
      return null;
    }
  },

  setSetting(key: string, value: string): Promise<SettingResponse> {
    return post<SetSettingBody, SettingResponse>('/settings', { key, value });
  },

  getLanguage(): Promise<{ language: string }> {
    return get<{ language: string }>('/config/language');
  },

  setLanguage(language: string): Promise<{ language: string }> {
    return post<{ language: string }, { language: string }>('/config/language', { language });
  },

  serviceStop(project: string, name: string, service: string): Promise<SuccessResponse> {
    return post<ServiceControlRequest, SuccessResponse>('/service/stop', { project, name, service });
  },

  serviceStart(project: string, name: string, service: string): Promise<SuccessResponse> {
    return post<ServiceControlRequest, SuccessResponse>('/service/start', { project, name, service });
  },

  serviceRestart(project: string, name: string, service: string): Promise<SuccessResponse> {
    return post<ServiceControlRequest, SuccessResponse>('/service/restart', { project, name, service });
  },

  bareServiceStop(project: string, name: string, service: string): Promise<SuccessResponse> {
    return post<ServiceControlRequest, SuccessResponse>('/bare-service/stop', { project, name, service });
  },

  bareServiceStart(project: string, name: string, service: string): Promise<SuccessResponse> {
    return post<ServiceControlRequest, SuccessResponse>('/bare-service/start', { project, name, service });
  },

  bareServiceRestart(project: string, name: string, service: string): Promise<SuccessResponse> {
    return post<ServiceControlRequest, SuccessResponse>('/bare-service/restart', { project, name, service });
  },

  portHealth(project: string, name: string): Promise<{ ports: PortHealthStatus[] }> {
    return post<{ project: string; name: string }, { ports: PortHealthStatus[] }>('/port-health', { project, name });
  },

  serviceRm(project: string, name: string, service: string): Promise<SuccessResponse> {
    return post<ServiceControlRequest, SuccessResponse>('/service/rm', { project, name, service });
  },

  listImages(project: ProjectName, name: InstanceName): Promise<readonly ImageSummary[]> {
    return get<readonly ImageSummary[]>(`/images?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}`);
  },

  listSecrets(project: ProjectName, name: InstanceName): Promise<readonly SecretInfo[]> {
    return get<readonly SecretInfo[]>(`/secrets?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}`);
  },

  revealSecret(project: ProjectName, name: InstanceName, secret: string): Promise<RevealSecretResponse> {
    return get<RevealSecretResponse>(`/secrets/reveal?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}&secret=${encodeURIComponent(secret)}`);
  },

  overrideSecret(project: ProjectName, name: InstanceName, secret: string, value: string): Promise<unknown> {
    return post<{ action: string; instance: string; project: string; name: string; value: string }, unknown>(
      '/secret',
      { action: 'Set', instance: name as string, project: project as string, name: secret, value },
    );
  },

  rerunExtractors(
    project: string,
    buildId?: string | null,
    onProgress?: (event: BuildProgressEvent) => void,
  ): Promise<{ complete?: RerunExtractorsResponse; error?: { error: string } }> {
    return consumeSSE<BuildProgressEvent, RerunExtractorsResponse>(
      '/api/v1/stream/rerun-extractors',
      { project, build_id: buildId },
      onProgress,
    );
  },

  inspectImage(project: ProjectName, name: InstanceName, image: string): Promise<ImageInspectResponse> {
    return get<ImageInspectResponse>(`/images/inspect?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}&image=${encodeURIComponent(image)}`);
  },

  listVolumes(project: ProjectName, name: InstanceName): Promise<readonly VolumeSummaryResponse[]> {
    return get<readonly VolumeSummaryResponse[]>(`/volumes?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}`);
  },

  inspectVolume(project: ProjectName, name: InstanceName, volume: string): Promise<VolumeInspectResponse> {
    return get<VolumeInspectResponse>(`/volumes/inspect?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}&volume=${encodeURIComponent(volume)}`);
  },

  serviceInspect(project: string, name: string, service: string): Promise<ServiceInspectResponse> {
    return get<ServiceInspectResponse>(`/service/inspect?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}&service=${encodeURIComponent(service)}`);
  },

  fileTree(project: string, name: string, path: string): Promise<readonly FileEntry[]> {
    return get<readonly FileEntry[]>(
      `/files/tree?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}&path=${encodeURIComponent(path)}`,
    );
  },

  fileRead(project: string, name: string, path: string): Promise<FileReadResponse> {
    return get<FileReadResponse>(
      `/files/read?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}&path=${encodeURIComponent(path)}`,
    );
  },

  fileWrite(project: string, name: string, path: string, content: string): Promise<SuccessResponse> {
    return post<FilesWriteBody, SuccessResponse>(
      '/files/write',
      { project, name, path, content },
    );
  },

  fileSearch(project: string, name: string, query: string): Promise<readonly string[]> {
    return get<readonly string[]>(
      `/files/search?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}&query=${encodeURIComponent(query)}`,
    );
  },

  fileIndex(project: string, name: string): Promise<readonly string[]> {
    return get<readonly string[]>(
      `/files/index?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}`,
    );
  },

  fileGitStatus(project: string, name: string): Promise<readonly GitFileStatus[]> {
    return get<readonly GitFileStatus[]>(
      `/files/git-status?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}`,
    );
  },

  fileGrep(project: string, name: string, query: string, regex?: boolean): Promise<readonly GrepMatch[]> {
    const r = regex ? '&regex=true' : '';
    return get<readonly GrepMatch[]>(
      `/files/grep?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}&query=${encodeURIComponent(query)}${r}`,
    );
  },

  hostServiceInspect(project: string, service: string): Promise<unknown> {
    return get<unknown>(`/host-service/inspect?project=${encodeURIComponent(project)}&service=${encodeURIComponent(service)}`);
  },

  hostImageInspect(project: string, image: string): Promise<unknown> {
    return get<unknown>(`/host-image/inspect?project=${encodeURIComponent(project)}&image=${encodeURIComponent(image)}`);
  },

  sharedLs(project: string): Promise<SharedResponse> {
    return get<SharedResponse>(`/shared/ls?project=${encodeURIComponent(project)}`);
  },

  sharedLsAll(): Promise<SharedAllResponse> {
    return get<SharedAllResponse>('/shared/ls-all');
  },


  sharedStartAll(project: string): Promise<SharedResponse> {
    return post<{ action: string; project: string }, SharedResponse>('/shared', { action: 'Start', project });
  },

  sharedStop(project: string, service: string): Promise<SharedResponse> {
    return post<{ action: string; project: string; service: string }, SharedResponse>('/shared', { action: 'Stop', project, service });
  },

  sharedStart(project: string, service: string): Promise<SharedResponse> {
    return post<{ action: string; project: string; service: string }, SharedResponse>('/shared', { action: 'Start', project, service });
  },

  sharedRestart(project: string, service: string): Promise<SharedResponse> {
    return post<{ action: string; project: string; service: string }, SharedResponse>('/shared', { action: 'Restart', project, service });
  },

  sharedRm(project: string, service: string): Promise<SharedResponse> {
    return post<{ action: string; project: string; service: string }, SharedResponse>('/shared', { action: 'Rm', project, service });
  },

  assignInstance(
    project: string,
    name: string,
    worktree: string,
    commitSha?: string,
    onProgress?: (event: BuildProgressEvent) => void,
  ): Promise<{ complete?: unknown; error?: { error: string } }> {
    return consumeSSE<BuildProgressEvent, unknown>(
      '/api/v1/stream/assign',
      { name, project, worktree, commit_sha: commitSha },
      onProgress,
    );
  },

  unassignInstance(
    project: string,
    name: string,
    onProgress?: (event: BuildProgressEvent) => void,
  ): Promise<{ complete?: unknown; error?: { error: string } }> {
    return consumeSSE<BuildProgressEvent, unknown>(
      '/api/v1/stream/unassign',
      { name, project },
      onProgress,
    );
  },

  runInstance(
    project: string,
    name: string,
    worktree?: string,
    buildId?: string,
    coastfileType?: string | null,
    forceRemoveDangling?: boolean,
    onProgress?: (event: BuildProgressEvent) => void,
    branch?: string | null,
    remote?: string | null,
  ): Promise<{ complete?: unknown; error?: { error: string } }> {
    return consumeSSE<BuildProgressEvent, unknown>(
      '/api/v1/stream/run',
      {
        name,
        project,
        branch: branch ?? undefined,
        worktree,
        build_id: buildId,
        coastfile_type: coastfileType,
        force_remove_dangling: forceRemoveDangling ?? false,
        remote: remote ?? undefined,
      },
      onProgress,
    );
  },

  buildProject(
    coastfilePath: string,
    refresh: boolean,
    onProgress?: (event: BuildProgressEvent) => void,
  ): Promise<{ complete?: unknown; error?: { error: string } }> {
    return consumeSSE<BuildProgressEvent, unknown>(
      '/api/v1/stream/build',
      { coastfile_path: coastfilePath, refresh },
      onProgress,
    );
  },

  buildsLs(project?: string): Promise<{ kind: string; builds: BuildSummary[] }> {
    const params = project ? `?project=${encodeURIComponent(project)}` : '';
    return get(`/builds${params}`);
  },

  buildsCoastfileTypes(project: string): Promise<CoastfileTypesResponse> {
    return get<CoastfileTypesResponse>(`/builds/coastfile-types?project=${encodeURIComponent(project)}`);
  },

  buildsInspect(project: string, buildId?: string): Promise<BuildsInspectResponse> {
    const bid = buildId ? `&build_id=${encodeURIComponent(buildId)}` : '';
    return get<BuildsInspectResponse>(`/builds/inspect?project=${encodeURIComponent(project)}${bid}`);
  },

  buildsImages(project: string, buildId?: string): Promise<BuildsImagesResponse> {
    const bid = buildId ? `&build_id=${encodeURIComponent(buildId)}` : '';
    return get<BuildsImagesResponse>(`/builds/images?project=${encodeURIComponent(project)}${bid}`);
  },

  buildsDockerImages(project: string, buildId?: string): Promise<BuildsDockerImagesResponse> {
    const bid = buildId ? `&build_id=${encodeURIComponent(buildId)}` : '';
    return get<BuildsDockerImagesResponse>(`/builds/docker-images?project=${encodeURIComponent(project)}${bid}`);
  },

  buildsCompose(project: string, buildId?: string): Promise<BuildsContentResponse> {
    const bid = buildId ? `&build_id=${encodeURIComponent(buildId)}` : '';
    return get<BuildsContentResponse>(`/builds/compose?project=${encodeURIComponent(project)}${bid}`);
  },

  buildsCoastfile(project: string, buildId?: string): Promise<BuildsContentResponse> {
    const bid = buildId ? `&build_id=${encodeURIComponent(buildId)}` : '';
    return get<BuildsContentResponse>(`/builds/coastfile?project=${encodeURIComponent(project)}${bid}`);
  },

  mcpLs(project: string, name: string): Promise<McpLsResponse> {
    return get<McpLsResponse>(`/mcp/ls?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}`);
  },

  mcpTools(project: string, name: string, server: string, tool?: string): Promise<McpToolsResponse> {
    let url = `/mcp/tools?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}&server=${encodeURIComponent(server)}`;
    if (tool) url += `&tool=${encodeURIComponent(tool)}`;
    return get<McpToolsResponse>(url);
  },

  mcpLocations(project: string, name: string): Promise<McpLocationsResponse> {
    return get<McpLocationsResponse>(`/mcp/locations?project=${encodeURIComponent(project)}&name=${encodeURIComponent(name)}`);
  },

  docsSearch(
    query: string,
    language?: string,
    limit?: number,
  ): Promise<DocsSearchResponse> {
    const params = new URLSearchParams({ q: query });
    if (language != null) params.set('language', language);
    if (limit != null) params.set('limit', String(limit));
    return get<DocsSearchResponse>(`/docs/search?${params.toString()}`);
  },

  dockerInfo(): Promise<DockerInfoResponse> {
    return get<DockerInfoResponse>('/docker/info');
  },

  openDockerSettings(): Promise<OpenDockerSettingsResponse> {
    return post<Record<string, never>, OpenDockerSettingsResponse>(
      '/docker/open-settings',
      {},
    );
  },

  checkUpdate(): Promise<UpdateCheckResponse> {
    return get<UpdateCheckResponse>('/update/check');
  },

  remotesLs(): Promise<RemoteResponse> {
    return get<RemoteResponse>('/remotes');
  },

  remoteStats(): Promise<RemoteStatsResponse> {
    return get<RemoteStatsResponse>('/remotes/stats');
  },

  remoteArch(name: string): Promise<{ arch: string }> {
    return get<{ arch: string }>(`/remotes/arch?name=${encodeURIComponent(name)}`);
  },

  remoteBuild(
    remote: string,
    coastfilePath: string,
    refresh: boolean,
    onProgress?: (event: BuildProgressEvent) => void,
  ): Promise<{ complete?: unknown; error?: { error: string } }> {
    return consumeSSE<BuildProgressEvent, unknown>(
      '/api/v1/stream/remote-build',
      { coastfile_path: coastfilePath, refresh, remote },
      onProgress,
    );
  },

  sshKeysLs(): Promise<SshKeysResponse> {
    return get<SshKeysResponse>('/ssh-keys');
  },

  sshKeyValidate(path: string): Promise<SshKeyValidateResponse> {
    return get<SshKeyValidateResponse>(`/ssh-keys/validate?path=${encodeURIComponent(path)}`);
  },

  remoteRm(name: string): Promise<RemoteResponse> {
    return post<{ action: string; name: string }, RemoteResponse>(
      '/remote/rm',
      { action: 'Rm', name },
    );
  },

  remoteAdd(params: {
    name: string;
    host: string;
    user: string;
    port: number;
    ssh_key: string | null;
    sync_strategy: string;
  }): Promise<RemoteResponse> {
    return post<{ action: string; name: string; host: string; user: string; port: number; ssh_key: string | null; sync_strategy: string }, RemoteResponse>(
      '/remote/add',
      { action: 'Add', ...params },
    );
  },

  applyUpdate(): Promise<UpdateApplyResponse> {
    return post<Record<string, never>, UpdateApplyResponse>('/update/apply', {});
  },

  /** Fire-and-forget analytics event. */
  track(event: string, metadata?: AnalyticsMetadata): void {
    beacon('/analytics/track', { event, url: window.location.href, metadata });
  },
} as const;
