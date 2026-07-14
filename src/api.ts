import { invoke } from "@tauri-apps/api/core";
import type { AppSnapshot, ComponentId } from "./types";

export const isTauri = () => Boolean(window.__TAURI_INTERNALS__);

const demoSnapshot: AppSnapshot = {
  platform: "Windows",
  architecture: "x86_64",
  manager_config_directory: "C:\\Users\\User\\.cpamanager-native",
  data_directory: "C:\\Users\\User\\AppData\\Roaming\\CPA Manager Native",
  launch_at_startup: false,
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
      management_url: "http://127.0.0.1:8317",
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
