import { invoke } from "@tauri-apps/api/core";
import type { AppSnapshot, ComponentId, ProviderModelSyncSettings, WebDavSettings } from "./types";

export const isTauri = () => Boolean(window.__TAURI_INTERNALS__);

const demoSnapshot: AppSnapshot = {
  platform: "Windows",
  architecture: "x86_64",
  manager_config_directory: "C:\\Users\\User\\.cpamanager-native",
  data_directory: "C:\\Users\\User\\AppData\\Roaming\\CPA Manager Native",
  launch_at_startup: false,
  lan_access_enabled: false,
  webdav: {
    base_url: "", username: "", password: "",
    remote_path: "CPA-Manager-Native/config-backup.zip",
    last_sync_at: null, last_error: null,
  },
  provider_model_sync: { enabled: true, interval_seconds: 300, alias_rules: [] },
  provider_model_sync_last_success: null,
  provider_model_sync_last_error: null,
  last_update_check: null,
  components: [
    {
      id: "cliproxyapi",
      name: "CLIProxyAPI",
      short_name: "CPA Core",
      description: "本地 AI API 网关与协议转换核心",
      repository: "router-for-me/CLIProxyAPI",
      installed_version: null,
      latest_version: null,
      lifecycle: "not_installed",
      healthy: false,
      pid: null,
      port: 8317,
      management_url: "http://127.0.0.1:8317/management.html",
      management_key: "cpa_demo_key_generated_on_first_start",
      update_available: false,
      busy: false,
      progress_percent: null,
      progress_label: null,
      last_error: null,
    },
    {
      id: "cpa-manager-plus",
      name: "CPA-Manager-Plus",
      short_name: "CPAMP",
      description: "CLIProxyAPI 管理、监控与可视化控制台",
      repository: "seakee/CPA-Manager-Plus",
      installed_version: null,
      latest_version: null,
      lifecycle: "not_installed",
      healthy: false,
      pid: null,
      port: 18317,
      management_url: "http://127.0.0.1:18317",
      management_key: null,
      update_available: false,
      busy: false,
      progress_percent: null,
      progress_label: null,
      last_error: null,
    },
  ],
  logs: [
    {
      timestamp: new Date().toISOString(),
      component_id: "app",
      level: "info",
      message: "浏览器预览模式：连接 Tauri 后端后可执行真实操作。",
    },
  ],
};

export async function getSnapshot(): Promise<AppSnapshot> {
  return isTauri() ? invoke<AppSnapshot>("get_app_snapshot") : demoSnapshot;
}

export async function runAppCommand(
  command: string,
  componentId?: ComponentId,
): Promise<AppSnapshot> {
  if (!isTauri()) {
    await new Promise((resolve) => setTimeout(resolve, 350));
    return demoSnapshot;
  }

  return invoke<AppSnapshot>(command, componentId ? { componentId } : undefined);
}

export async function selectDataDirectory(): Promise<string | null> {
  if (!isTauri()) {
    await new Promise((resolve) => setTimeout(resolve, 250));
    return "D:\\CPA-Manager-Data";
  }

  return invoke<string | null>("select_data_directory");
}

export async function changeDataDirectory(directory: string): Promise<AppSnapshot> {
  if (!isTauri()) {
    await new Promise((resolve) => setTimeout(resolve, 500));
    demoSnapshot.data_directory = directory;
    return demoSnapshot;
  }

  return invoke<AppSnapshot>("change_data_directory", { directory });
}

export async function setLaunchAtStartup(enabled: boolean): Promise<AppSnapshot> {
  if (!isTauri()) {
    await new Promise((resolve) => setTimeout(resolve, 250));
    demoSnapshot.launch_at_startup = enabled;
    return demoSnapshot;
  }

  return invoke<AppSnapshot>("set_launch_at_startup", { enabled });
}

export async function setLanAccess(enabled: boolean): Promise<AppSnapshot> {
  if (!isTauri()) {
    await new Promise((resolve) => setTimeout(resolve, 250));
    demoSnapshot.lan_access_enabled = enabled;
    return demoSnapshot;
  }

  return invoke<AppSnapshot>("set_lan_access", { enabled });
}

export async function saveWebDavSettings(settings: WebDavSettings): Promise<AppSnapshot> {
  if (!isTauri()) { demoSnapshot.webdav = settings; return demoSnapshot; }
  return invoke<AppSnapshot>("save_webdav_settings", { settings });
}

export async function testWebDavConnection(settings: WebDavSettings): Promise<void> {
  if (!isTauri()) { await new Promise((resolve) => setTimeout(resolve, 300)); return; }
  return invoke<void>("test_webdav_connection", { settings });
}

export async function syncWebDav(direction: "upload" | "download"): Promise<AppSnapshot> {
  if (!isTauri()) { await new Promise((resolve) => setTimeout(resolve, 500)); return demoSnapshot; }
  return invoke<AppSnapshot>(direction === "upload" ? "upload_webdav_config" : "download_webdav_config");
}

export async function saveProviderModelSyncSettings(
  settings: ProviderModelSyncSettings,
): Promise<AppSnapshot> {
  if (!isTauri()) {
    demoSnapshot.provider_model_sync = settings;
    return demoSnapshot;
  }
  return invoke<AppSnapshot>("save_provider_model_sync_settings", { settings });
}

export async function syncProviderModelsNow(): Promise<AppSnapshot> {
  if (!isTauri()) {
    demoSnapshot.provider_model_sync_last_success = new Date().toISOString();
    return demoSnapshot;
  }
  return invoke<AppSnapshot>("sync_provider_models_now");
}
