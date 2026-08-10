export type ComponentId = "cliproxyapi" | "cpa-manager-plus";

export type LifecycleState =
  | "not_installed"
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "installing"
  | "updating"
  | "error";

export interface ComponentSnapshot {
  id: ComponentId;
  name: string;
  short_name: string;
  description: string;
  repository: string;
  installed_version: string | null;
  latest_version: string | null;
  lifecycle: LifecycleState;
  healthy: boolean;
  pid: number | null;
  port: number;
  management_url: string | null;
  management_key: string | null;
  update_available: boolean;
  busy: boolean;
  progress_percent: number | null;
  progress_label: string | null;
  last_error: string | null;
}

export interface LogEntry {
  timestamp: string;
  component_id: ComponentId | "app";
  level: "info" | "warn" | "error";
  message: string;
}

export interface AppSnapshot {
  platform: string;
  architecture: string;
  manager_config_directory: string;
  data_directory: string;
  launch_at_startup: boolean;
  lan_access_enabled: boolean;
  webdav: WebDavSettings;
  provider_model_sync: ProviderModelSyncSettings;
  provider_model_sync_last_success: string | null;
  provider_model_sync_last_error: string | null;
  last_update_check: string | null;
  components: ComponentSnapshot[];
  logs: LogEntry[];
}

export interface ProviderModelAliasRule {
  enabled: boolean;
  provider_pattern: string;
  model_pattern: string;
  alias_replacement: string;
  force_mapping: boolean;
}

export interface ProviderModelSyncSettings {
  enabled: boolean;
  interval_seconds: number;
  alias_rules: ProviderModelAliasRule[];
}

export interface WebDavSettings {
  base_url: string;
  username: string;
  password: string;
  remote_path: string;
  last_sync_at: string | null;
  last_error: string | null;
}
