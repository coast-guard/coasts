/**
 * Re-exports all API types from the auto-generated bindings, plus a handful
 * of TS-only helper types that have no Rust counterpart.
 *
 * The canonical type definitions live in ./generated/ and are produced by
 * `ts-rs` from the Rust serde structs.
 *
 * Run `npm run generate:types` to regenerate after changing Rust types.
 */

// Re-export every generated type so existing `from '../types/api'` imports
// continue to work unchanged.
export type {
  // CheckoutRequest intentionally omitted — overridden below with branded types.
  ActivateAgentShellResponse,
  AgentShellAvailableResponse,
  ArchiveProjectResponse,
  BuildProgressEvent,
  BuildSummary,
  BuildsContentResponse,
  BuildsDockerImagesResponse,
  BuildsImagesResponse,
  BuildsInspectResponse,
  CachedImageInfo,
  CheckoutResponse,
  ClearLogsResponse,
  CloseAgentShellResponse,
  CoastEvent,
  CoastfileTypesResponse,
  ContainerStats,
  DockerImageInfo,
  DockerInfoResponse,
  ErrorResponse,
  ExecSessionInfo,
  FileEntry,
  FileReadResponse,
  GetSettingResponse,
  GitFileStatus,
  GrepMatch,
  HostServiceSessionInfo,
  ImageInspectResponse,
  ImageSummary,
  InstanceStatus,
  InstanceSummary,
  KnownProject,
  LogsResponse,
  LsResponse,
  McpLocationSummary,
  McpLocationsResponse,
  McpLsResponse,
  McpServerSummary,
  McpToolInfo,
  McpToolSummary,
  McpToolsResponse,
  PortMapping,
  PortsResponse,
  PortHealthStatus,
  ProjectGitResponse,
  ProjectSharedSummary,
  PsResponse,
  RemoteEntry,
  RemoteResponse,
  RemoteStats,
  RemoteStatsResponse,
  RerunExtractorsResponse,
  RestartServicesResponse,
  RevealSecretResponse,
  RmBuildRequest,
  RmBuildResponse,
  RmResponse,
  RuntimeType,
  SecretInfo,
  ServiceExecSessionInfo,
  ServiceInspectResponse,
  ServiceStatus,
  SessionInfo,
  SettingResponse,
  SharedAllResponse,
  SharedResponse,
  SharedServiceBuildInfo,
  SharedServiceInfo,
  SpawnAgentShellResponse,
  SshKeyEntry,
  SshKeyValidateResponse,
  SshKeysResponse,
  StartResponse,
  StopResponse,
  SuccessResponse,
  UnarchiveProjectResponse,
  UploadResponse,
  VolumeBuildInfo,
  VolumeInspectResponse,
  VolumeSummaryResponse,

  OpenDockerSettingsResponse,
  UpdateCheckResponse,
  UpdateApplyResponse,
} from './generated/index';

// ---------------------------------------------------------------------------
// TS-only types that have no Rust counterpart.
// These were previously generated but removed from the Rust side; kept here
// because endpoints.ts still uses them as request-body type parameters.
// ---------------------------------------------------------------------------

import type { ProjectName, InstanceName } from './branded';

export interface AgentShellActionRequest {
  readonly project: string;
  readonly name: string;
  readonly shell_id: number;
}

export interface SpawnAgentShellRequest {
  readonly project: string;
  readonly name: string;
}

export interface FilesWriteBody {
  readonly project: string;
  readonly name: string;
  readonly path: string;
  readonly content: string;
}

export interface ServiceControlRequest {
  readonly project: string;
  readonly name: string;
  readonly service: string;
}

export interface SetSettingBody {
  readonly key: string;
  readonly value: string;
}

export interface CheckoutRequest {
  readonly name?: InstanceName | undefined;
  readonly project: ProjectName;
}

export interface NameProjectRequest {
  readonly name: InstanceName;
  readonly project: ProjectName;
}

export interface LogsRequest extends NameProjectRequest {
  readonly service: string | null;
  readonly tail?: number | null;
  readonly tail_all?: boolean;
  readonly follow: boolean;
}

export interface CoastfileVolumeConfig {
  readonly name: string;
  readonly strategy: 'isolated' | 'shared';
  readonly service: string;
  readonly mount: string;
  readonly snapshot_source: string | null;
}
