use std::{fmt, path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::error::{message, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentId {
    Cliproxyapi,
    CpaManagerPlus,
    Octopus,
}

impl ComponentId {
    pub const ALL: [Self; 3] = [Self::Cliproxyapi, Self::CpaManagerPlus, Self::Octopus];

    pub fn directory_name(self) -> &'static str {
        match self {
            Self::Cliproxyapi => "cliproxyapi",
            Self::CpaManagerPlus => "cpa-manager-plus",
            Self::Octopus => "octopus",
        }
    }
}

impl fmt::Display for ComponentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.directory_name())
    }
}

impl FromStr for ComponentId {
    type Err = crate::error::AppError;

    fn from_str(value: &str) -> AppResult<Self> {
        match value {
            "cliproxyapi" => Ok(Self::Cliproxyapi),
            "cpa-manager-plus" => Ok(Self::CpaManagerPlus),
            "octopus" => Ok(Self::Octopus),
            _ => Err(message(format!("未知组件：{value}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    NotInstalled,
    Stopped,
    Starting,
    Running,
    Stopping,
    Installing,
    Updating,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentSnapshot {
    pub id: ComponentId,
    pub name: String,
    pub short_name: String,
    pub description: String,
    pub repository: String,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub lifecycle: LifecycleState,
    pub healthy: bool,
    pub pid: Option<u32>,
    pub port: u16,
    pub auto_start: bool,
    pub management_url: Option<String>,
    pub management_key: Option<String>,
    pub update_available: bool,
    pub busy: bool,
    pub progress_percent: Option<u8>,
    pub progress_label: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppSnapshot {
    pub platform: String,
    pub architecture: String,
    pub manager_config_directory: String,
    pub data_directory: String,
    pub github_release_download_proxy: String,
    pub launch_at_startup: bool,
    pub lan_access_enabled: bool,
    pub webdav: crate::config::WebDavSettings,
    pub provider_model_sync: crate::config::ProviderModelSyncSettings,
    pub provider_model_sync_last_success: Option<String>,
    pub provider_model_sync_last_error: Option<String>,
    pub last_update_check: Option<String>,
    pub components: Vec<ComponentSnapshot>,
    pub logs: Vec<LogEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub component_id: LogSource,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogSource {
    App,
    Cliproxyapi,
    CpaManagerPlus,
    Octopus,
}

impl From<ComponentId> for LogSource {
    fn from(value: ComponentId) -> Self {
        match value {
            ComponentId::Cliproxyapi => Self::Cliproxyapi,
            ComponentId::CpaManagerPlus => Self::CpaManagerPlus,
            ComponentId::Octopus => Self::Octopus,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallManifest {
    pub version: String,
    pub asset_name: String,
    pub download_url: String,
    pub sha256: String,
    pub installed_at: String,
    pub executable_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RuntimeState {
    pub installed: Option<InstallManifest>,
    pub latest_version: Option<String>,
    pub lifecycle: LifecycleState,
    pub healthy: bool,
    pub pid: Option<u32>,
    pub busy: bool,
    pub progress_percent: Option<u8>,
    pub progress_label: Option<String>,
    pub last_error: Option<String>,
    pub management_key: Option<String>,
}

impl RuntimeState {
    pub fn new(installed: Option<InstallManifest>) -> Self {
        Self {
            lifecycle: if installed.is_some() {
                LifecycleState::Stopped
            } else {
                LifecycleState::NotInstalled
            },
            installed,
            latest_version: None,
            healthy: false,
            pid: None,
            busy: false,
            progress_percent: None,
            progress_label: None,
            last_error: None,
            management_key: None,
        }
    }
}
