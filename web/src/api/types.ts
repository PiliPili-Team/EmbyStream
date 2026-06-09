export type UserRole = "admin" | "user";
export type StreamMode = "frontend" | "backend" | "dual";
export type DraftStatus = "draft" | "generated" | "archived";
export type ArtifactType =
  | "config_toml"
  | "nginx_conf"
  | "docker_compose"
  | "systemd_service"
  | "pm2_config";

export interface SessionUser {
  id: string;
  username: string;
  email: string | null;
  role: UserRole;
}

export interface AdminUserSummary {
  id: string;
  username: string;
  email: string | null;
  role: UserRole;
  disabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface AuthResponse {
  user: SessionUser;
}

export interface RegistrationSettingsResponse {
  registration_enabled: boolean;
}

export interface RegisterRequest {
  username: string;
  email?: string | null;
  password: string;
}

export interface BackgroundItem {
  image_url: string;
  title: string;
  subtitle?: string | null;
}

export interface LoginBackgroundResponse {
  provider: "tmdb" | "bing" | "static_fallback";
  fetched_at: string;
  expires_at: string;
  items: BackgroundItem[];
}

export interface PathRewriteConfig {
  enable: boolean;
  pattern: string;
  replacement: string;
}

export interface AntiReverseProxyConfig {
  enable: boolean;
  /**
   * Trusted hosts allowed to access the proxied service. Multiple domains are
   * supported; the backend also accepts the legacy single-string form on read.
   */
  host: string[];
}

export type BackendNodeType =
  | "Disk"
  | "OpenList"
  | "DirectLink"
  | "googleDrive"
  | "WebDav";

export interface DiskNodeConfig {
  description: string;
}

export interface OpenListNodeConfig {
  base_url: string;
  port: string;
  token: string;
}

export interface DirectLinkNodeConfig {
  user_agent: string;
}

export interface GoogleDriveNodeConfig {
  node_uuid: string;
  client_id: string;
  client_secret: string;
  drive_id: string;
  drive_name: string;
  access_token: string;
  refresh_token: string;
}

export interface WebDavNodeConfig {
  url_mode: string;
  node_uuid: string;
  query_param: string;
  url_template: string;
  username: string;
  password: string;
  user_agent: string;
}

export interface BackendNodeConfig {
  name: string;
  backend_type: BackendNodeType | string;
  pattern: string;
  base_url: string;
  port: string;
  path: string;
  priority: number;
  proxy_mode: string;
  client_speed_limit_kbs: number;
  client_burst_speed_kbs: number;
  path_rewrites: PathRewriteConfig[];
  anti_reverse_proxy: AntiReverseProxyConfig;
  disk: DiskNodeConfig | null;
  open_list: OpenListNodeConfig | null;
  direct_link: DirectLinkNodeConfig | null;
  google_drive: GoogleDriveNodeConfig | null;
  webdav: WebDavNodeConfig | null;
}

export interface NginxFrontendConfig {
  server_name: string;
  ssl_certificate: string;
  ssl_certificate_key: string;
  client_max_body_size: string;
  static_location_pattern: string;
  websocket_location_pattern: string;
}

export interface NginxBackendConfig {
  server_name: string;
  ssl_certificate: string;
  ssl_certificate_key: string;
  client_max_body_size: string;
  resolver_provider: string;
  custom_resolvers: string;
  access_log_path: string;
  error_log_path: string;
  google_drive_access_log_path: string;
}

export interface NginxConfigPayload {
  frontend: NginxFrontendConfig;
  backend: NginxBackendConfig;
}

export interface SystemdDeploymentConfig {
  binary_path: string;
  working_directory: string;
  config_path: string;
}

export interface Pm2DeploymentConfig {
  binary_path: string;
  working_directory: string;
  config_path: string;
  out_file: string;
  error_file: string;
}

export interface DeploymentConfigPayload {
  systemd: SystemdDeploymentConfig;
  pm2: Pm2DeploymentConfig;
}

export interface WizardPayload {
  stream_mode: StreamMode;
  shared: {
    log: {
      level: string;
      prefix: string;
      root_path: string;
    };
    general: {
      memory_mode: string;
      encipher_key: string;
      encipher_iv: string;
    };
    emby: {
      url: string;
      port: string;
      token: string;
    };
    user_agent: {
      mode: string;
      allow_ua: string[];
      deny_ua: string[];
    };
    fallback: {
      video_missing_path: string;
    };
    http2: {
      ssl_cert_file: string;
      ssl_key_file: string;
    };
  };
  frontend: null | {
    listen_port: number;
    path_rewrites: PathRewriteConfig[];
    anti_reverse_proxy: AntiReverseProxyConfig;
  };
  backend: null | {
    listen_port: number;
    base_url: string;
    port: string;
    path: string;
    problematic_clients: string[];
  };
  backend_nodes: BackendNodeConfig[];
  nginx: NginxConfigPayload;
  deployment: DeploymentConfigPayload;
}

export interface DraftSummary {
  id: string;
  name: string;
  status: DraftStatus;
  stream_mode: StreamMode;
  updated_at: string;
}

export interface DraftEnvelope {
  draft: DraftSummary;
}

export interface MetadataUpdateRequest {
  name: string;
}

export interface DraftListResponse {
  items: DraftSummary[];
}

export interface DraftDocument {
  id: string;
  name: string;
  status: DraftStatus;
  stream_mode: StreamMode;
  payload: WizardPayload;
  updated_at: string;
}

export interface DraftDocumentEnvelope {
  draft: DraftDocument;
}

export interface WizardTemplateResponse {
  payload: WizardPayload;
}

export interface SaveDraftRequest {
  name: string;
  payload: WizardPayload;
  client_revision: number;
}

export interface ConfigSetSummary {
  id: string;
  name: string;
  stream_mode: StreamMode;
  created_at: string;
  updated_at: string;
}

export interface ConfigSetListResponse {
  items: ConfigSetSummary[];
}

export interface ConfigSetEnvelope {
  config_set: ConfigSetSummary;
}

export interface ArtifactDocument {
  artifact_type: ArtifactType;
  file_name: string;
  language: string;
  content: string;
}

export interface ArtifactListResponse {
  items: ArtifactDocument[];
}

export interface GenerateDraftResponse {
  config_set: ConfigSetSummary;
  artifacts: Array<{
    artifact_type: ArtifactType;
    file_name: string;
  }>;
}

export interface LogListResponse {
  items: Array<{
    timestamp: string;
    level: string;
    source: string;
    message: string;
  }>;
  next_cursor?: string | null;
}

export interface LogStreamReplayMessage {
  kind: "replay";
  items: LogListResponse["items"];
}

export interface LogStreamEntryMessage {
  kind: "entry";
  item: LogListResponse["items"][number];
}

export type LogStreamMessage = LogStreamReplayMessage | LogStreamEntryMessage;

export interface UserListResponse {
  items: AdminUserSummary[];
}

export interface UserEnvelope {
  user: AdminUserSummary;
}

export interface UpdateRegistrationSettingsRequest {
  registration_enabled: boolean;
}

export interface SystemMetricsResponse {
  cpu_usage_percent: number;
  cpu_core_count: number;
  memory_used_bytes: number;
  memory_total_bytes: number;
  memory_usage_percent: number;
  disk_used_bytes: number;
  disk_total_bytes: number;
  disk_usage_percent: number;
  uptime_seconds: number;
}

export interface LogoutResponse {
  ok: boolean;
}
