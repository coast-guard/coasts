import type { ProjectName, InstanceName } from '../types/branded';
import type {
  SsgBuildInspectResponse,
  SsgBuildsLsHttpResponse,
  SsgBuildsRmResponse,
  SsgImagesHttpResponse,
  SsgStateResponse,
  SsgVolumesHttpResponse,
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

/**
 * Request body for the lifecycle endpoints `/api/v1/ssg/{run,start}`.
 * Mirrors `SsgLifecycleRequest` on the daemon side. Kept local so
 * the SPA does not need a generated TS binding for a one-field
 * struct.
 */
interface SsgLifecycleRequestBody {
  project: string;
}

/**
 * Lightweight client-side shape for the SsgResponse the daemon
 * returns from the lifecycle endpoints. The daemon emits a much
 * richer SsgResponse (services, ports, runtime status, …) but the
 * SPA only needs `status` + `message` to render toasts and trigger
 * a list refresh — the post-action state is fetched fresh via
 * {@link ssgState}.
 */
export interface SsgLifecycleHttpResponse {
  status: string;
  message: string;
}

/**
 * Request body for the per-service inner-compose endpoints
 * `/api/v1/ssg/services/{stop,start,restart,rm}`. Mirrors
 * `SsgServiceActionRequest` on the daemon side.
 */
interface SsgServiceActionRequestBody {
  project: string;
  service: string;
}

/**
 * Response shape for the per-service inner-compose endpoints.
 */
export interface SsgServiceActionHttpResponse {
  project: string;
  service: string;
  verb: string;
  message: string;
}

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

  /**
   * List SSG (Shared Service Group) build artifacts for `project`.
   * Backs the "SHARED SERVICE GROUPS" subsection on the project
   * detail page. Returns `{ project, builds: [] }` when the project
   * has no SSG builds yet (no error).
   */
  ssgBuildsLs(project: string): Promise<SsgBuildsLsHttpResponse> {
    return get<SsgBuildsLsHttpResponse>(
      `/ssg/builds?project=${encodeURIComponent(project)}`,
    );
  },

  /**
   * Inspect a single SSG build artifact: manifest contents +
   * raw `ssg-coastfile.toml` + `compose.yml`. Backs the per-SSG-
   * build detail page. Returns 404 when the build_id is unknown.
   */
  ssgBuildInspect(
    project: string,
    buildId: string,
  ): Promise<SsgBuildInspectResponse> {
    return get<SsgBuildInspectResponse>(
      `/ssg/builds/inspect?project=${encodeURIComponent(project)}&build_id=${encodeURIComponent(buildId)}`,
    );
  },

  /**
   * Combined SSG runtime view for `project`: container status,
   * services, port mapping (canonical + dynamic + virtual),
   * latest_build_id, and consumer pin. Backs the `/project/<p>/ssg`
   * SPA tab. Returns 200 with empty services + null status when the
   * project has no SSG yet — the SPA renders that as the "not built
   * yet" empty state.
   */
  ssgState(project: string): Promise<SsgStateResponse> {
    return get<SsgStateResponse>(
      `/ssg/state?project=${encodeURIComponent(project)}`,
    );
  },

  /**
   * List images inside the project's SSG outer DinD (i.e., the
   * inner Docker daemon's image set: postgres:15, redis:7, etc.).
   * Backs the SSG Images tab.
   */
  ssgImages(project: string): Promise<SsgImagesHttpResponse> {
    return get<SsgImagesHttpResponse>(
      `/ssg/images?project=${encodeURIComponent(project)}`,
    );
  },

  /**
   * Inspect a single image inside the project's SSG outer DinD.
   * Returns the full `docker inspect` JSON plus a list of
   * containers using this image. Same response shape as
   * {@link inspectImage} for instance-level images, so the SPA's
   * `ImageDetailPage` can be reused without modification.
   */
  ssgImageInspect(project: string, image: string): Promise<ImageInspectResponse> {
    return get<ImageInspectResponse>(
      `/ssg/images/inspect?project=${encodeURIComponent(project)}&image=${encodeURIComponent(image)}`,
    );
  },

  /**
   * List named volumes inside the project's SSG outer DinD (i.e.,
   * the inner Docker daemon's volumes: cg_postgres_data, etc.).
   * Backs the SSG Volumes tab.
   */
  ssgVolumes(project: string): Promise<SsgVolumesHttpResponse> {
    return get<SsgVolumesHttpResponse>(
      `/ssg/volumes?project=${encodeURIComponent(project)}`,
    );
  },

  /**
   * Inspect a single named volume inside the project's SSG outer
   * DinD. Returns `docker volume inspect` JSON, the containers
   * using it, and the matching `[shared_services.<svc>]` Coastfile
   * declaration if any. Same shape as {@link inspectVolume}.
   */
  ssgVolumeInspect(project: string, volume: string): Promise<VolumeInspectResponse> {
    return get<VolumeInspectResponse>(
      `/ssg/volumes/inspect?project=${encodeURIComponent(project)}&volume=${encodeURIComponent(volume)}`,
    );
  },

  /**
   * Inspect a single inner compose service running inside the
   * SSG outer DinD. Returns `docker inspect <inner_container>`
   * JSON. Same `ServiceInspectResponse` shape as
   * {@link serviceInspect} so the SPA's existing inspect-rendering
   * helpers can be reused.
   */
  ssgServiceInspect(project: string, service: string): Promise<ServiceInspectResponse> {
    return get<ServiceInspectResponse>(
      `/ssg/services/inspect?project=${encodeURIComponent(project)}&service=${encodeURIComponent(service)}`,
    );
  },

  /**
   * Remove SSG build artifacts. Pinned builds are skipped (never
   * deleted); missing artifact dirs are treated as already-removed
   * (idempotent). Response splits results into `removed`,
   * `skipped_pinned`, and `errors` so the SPA can surface partial
   * failures.
   */
  ssgBuildsRm(
    project: string,
    buildIds: readonly string[],
  ): Promise<SsgBuildsRmResponse> {
    return post<{ project: string; build_ids: readonly string[] }, SsgBuildsRmResponse>(
      '/ssg/builds/rm',
      { project, build_ids: buildIds },
    );
  },

  /**
   * Run the project's SSG (`docker create` + `docker start` of the
   * outer DinD). Idempotent: if the SSG is already running, returns
   * a no-op success. Mirrors `coast ssg run` from the CLI but
   * blocks until the operation completes (no SSE — progress events
   * are discarded server-side).
   */
  ssgRun(project: string): Promise<SsgLifecycleHttpResponse> {
    return post<SsgLifecycleRequestBody, SsgLifecycleHttpResponse>(
      '/ssg/run',
      { project },
    );
  },

  /**
   * Start a previously-stopped SSG. Reuses the existing
   * `ssg.container_id` and previously-allocated dynamic ports.
   * Returns 409 if no SSG record exists for the project.
   */
  ssgStart(project: string): Promise<SsgLifecycleHttpResponse> {
    return post<SsgLifecycleRequestBody, SsgLifecycleHttpResponse>(
      '/ssg/start',
      { project },
    );
  },

  /**
   * Stop the project's SSG (sends SIGSTOP to the outer DinD and
   * preserves the container so `Start` can resume it). When `force`
   * is true, tears down any remote-shadow tunnels first instead of
   * refusing if other instances reference the SSG.
   */
  ssgStop(
    project: string,
    options?: { force?: boolean },
  ): Promise<SsgLifecycleHttpResponse> {
    return post<
      { project: string; force: boolean },
      SsgLifecycleHttpResponse
    >('/ssg/stop', {
      project,
      force: Boolean(options?.force),
    });
  },

  /**
   * Remove the project's SSG (deletes the outer DinD container,
   * frees its virtual-port allocation, and clears the runtime row
   * from `ssg_services`). Build artifacts under `~/.coast/ssg/builds/`
   * are unaffected — use {@link ssgBuildsRm} for those. When
   * `with_data` is true, also drops inner named volumes (postgres
   * WAL etc.).
   */
  ssgRm(
    project: string,
    options?: { with_data?: boolean; force?: boolean },
  ): Promise<SsgLifecycleHttpResponse> {
    return post<
      { project: string; with_data: boolean; force: boolean },
      SsgLifecycleHttpResponse
    >('/ssg/rm', {
      project,
      with_data: Boolean(options?.with_data),
      force: Boolean(options?.force),
    });
  },

  /**
   * Restart every inner compose service inside the SSG outer
   * DinD without bouncing the outer DinD itself. Mirror of the
   * per-instance "Restart Services" action.
   */
  ssgRestartServices(project: string): Promise<{ message: string }> {
    return post<{ project: string }, { message: string }>(
      '/ssg/services/restart-all',
      { project },
    );
  },

  /**
   * Bind every SSG service's canonical port on the host. Maps to
   * `coast ssg checkout --all`. Toggle counterpart of
   * {@link ssgUncheckoutAll}.
   */
  ssgCheckoutAll(project: string): Promise<{ message: string }> {
    return post<{ project: string }, { message: string }>(
      '/ssg/checkout',
      { project },
    );
  },

  /**
   * Release every SSG service's canonical-port binding on the
   * host. Maps to `coast ssg uncheckout --all`. Toggle counterpart
   * of {@link ssgCheckoutAll}.
   */
  ssgUncheckoutAll(project: string): Promise<{ message: string }> {
    return post<{ project: string }, { message: string }>(
      '/ssg/uncheckout',
      { project },
    );
  },

  /**
   * Trigger an SSG build. Streams progress via SSE; resolves once
   * the build completes (or errors). Use `onProgress` to render
   * incremental status. Mirrors {@link buildProject} +
   * {@link remoteBuild} for the regular and remote build flows.
   */
  ssgBuild(
    project: string,
    workingDir?: string,
    file?: string,
    config?: string,
    onProgress?: (event: BuildProgressEvent) => void,
  ): Promise<{ complete?: unknown; error?: { error: string } }> {
    const body: {
      project: string;
      working_dir?: string;
      file?: string;
      config?: string;
    } = { project };
    if (workingDir != null) {
      body.working_dir = workingDir;
    }
    if (file != null) {
      body.file = file;
    }
    if (config != null) {
      body.config = config;
    }
    return consumeSSE<BuildProgressEvent, unknown>(
      '/api/v1/stream/ssg-build',
      body,
      onProgress,
    );
  },

  /**
   * Run a per-service compose verb (stop/start/restart/rm) on
   * one of the SSG's inner services (postgres, redis, etc.).
   * The daemon `docker exec`s into the outer DinD container and
   * runs `docker compose <verb> <service>` against the inner
   * compose stack. Backs the toolbar buttons on the SSG → Services
   * tab.
   */
  ssgServiceStop(
    project: string,
    service: string,
  ): Promise<SsgServiceActionHttpResponse> {
    return post<
      SsgServiceActionRequestBody,
      SsgServiceActionHttpResponse
    >('/ssg/services/stop', { project, service });
  },

  ssgServiceStart(
    project: string,
    service: string,
  ): Promise<SsgServiceActionHttpResponse> {
    return post<
      SsgServiceActionRequestBody,
      SsgServiceActionHttpResponse
    >('/ssg/services/start', { project, service });
  },

  ssgServiceRestart(
    project: string,
    service: string,
  ): Promise<SsgServiceActionHttpResponse> {
    return post<
      SsgServiceActionRequestBody,
      SsgServiceActionHttpResponse
    >('/ssg/services/restart', { project, service });
  },

  ssgServiceRm(
    project: string,
    service: string,
  ): Promise<SsgServiceActionHttpResponse> {
    return post<
      SsgServiceActionRequestBody,
      SsgServiceActionHttpResponse
    >('/ssg/services/rm', { project, service });
  },

  /**
   * Phase 33: drop every encrypted keystore entry whose
   * `coast_image == "ssg:<project>"`. Idempotent.
   *
   * Backs the SsgSecretsTab "Clear secrets" button. Mirrors the
   * `coast ssg secrets clear` CLI verb. The next `coast ssg run`
   * will start the SSG container but services that depend on a
   * missing env-var or file path will fail at compose-up time —
   * the user typically re-runs `coast ssg build` after a clear.
   *
   * See `coast-ssg/DESIGN.md §33`.
   */
  ssgSecretsClear(project: string): Promise<{ message: string }> {
    return post<{ project: string }, { message: string }>(
      '/ssg/secrets/clear',
      { project },
    );
  },

  /**
   * Phase 33: list every secret known to the SSG keystore for
   * `project`. Mirrors {@link listSecrets} for the per-project
   * SSG namespace (`ssg:<project>` + `ssg:<project>/override`).
   * Backs the SsgSecretsTab DataTable.
   */
  ssgListSecrets(project: string): Promise<readonly SecretInfo[]> {
    return get<readonly SecretInfo[]>(
      `/ssg/secrets?project=${encodeURIComponent(project)}`,
    );
  },

  /**
   * Phase 33: reveal a single SSG secret's plaintext value.
   * Override row wins over base row when both exist. Backs the
   * SsgSecretsTab eye-icon reveal modal.
   */
  ssgRevealSecret(
    project: string,
    secret: string,
  ): Promise<RevealSecretResponse> {
    return get<RevealSecretResponse>(
      `/ssg/secrets/reveal?project=${encodeURIComponent(project)}&secret=${encodeURIComponent(secret)}`,
    );
  },

  /**
   * Phase 33: write a user-supplied override into the SSG
   * keystore. Subsequent `coast ssg run` / `start` will inject
   * this value (the run-time materializer prefers overrides over
   * base values). Persists across `coast ssg build` since
   * rebuild only resets the base namespace. Backs the
   * SsgSecretsTab "Override" button.
   */
  ssgOverrideSecret(
    project: string,
    secret: string,
    value: string,
  ): Promise<{ message: string }> {
    return post<
      { project: string; name: string; value: string },
      { message: string }
    >('/ssg/secrets/override', { project, name: secret, value });
  },

  /**
   * Phase 33: re-run the SSG `[secrets.*]` extractor pass against
   * the cached `ssg-coastfile.toml` from the active build. Streams
   * a 2-step plan ("Resolving cached SSG Coastfile" + "Extracting
   * secrets") plus per-secret items. The user must restart the
   * SSG (stop + start) for refreshed values to propagate into the
   * inner compose stack. Backs the SsgSecretsTab "Re-run
   * extractors" button.
   */
  ssgRerunExtractors(
    project: string,
    onProgress?: (event: BuildProgressEvent) => void,
  ): Promise<{ complete?: RerunExtractorsResponse; error?: { error: string } }> {
    return consumeSSE<BuildProgressEvent, RerunExtractorsResponse>(
      '/api/v1/stream/ssg-rerun-extractors',
      { project },
      onProgress,
    );
  },

  /**
   * Run the project's SSG with progress streaming. Mirrors
   * {@link ssgBuild} for the run lifecycle: emits a sequence of
   * {@link BuildProgressEvent}s ("Preparing SSG", "Pulling image",
   * "Creating container", "Waiting for inner daemon", "Starting
   * inner services") and resolves with `{ complete }` on success
   * or `{ error }` on failure. Use this from the SSG Run modal to
   * render step progress; the synchronous {@link ssgRun} variant
   * exists for fire-and-forget use cases (e.g. the toolbar
   * button on the SSG list panel).
   */
  ssgRunStreaming(
    project: string,
    onProgress?: (event: BuildProgressEvent) => void,
  ): Promise<{ complete?: unknown; error?: { error: string } }> {
    return consumeSSE<BuildProgressEvent, unknown>(
      '/api/v1/stream/ssg-run',
      { project },
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
