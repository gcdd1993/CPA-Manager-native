use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Child,
    sync::{Arc, Mutex},
};

use chrono::Utc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    components::COMPONENTS,
    config::{
        cliproxy_lan_access_enabled, read_cliproxy_secret, write_manager_settings, ManagerSettings,
    },
    error::AppResult,
    github::is_update_available,
    models::{
        AppSnapshot, ComponentId, ComponentSnapshot, InstallManifest, LifecycleState, LogEntry,
        LogLevel, LogSource, RuntimeState,
    },
};

#[derive(Clone)]
pub struct AppState {
    manager_config_dir: Arc<PathBuf>,
    settings: Arc<Mutex<ManagerSettings>>,
    pub client: reqwest::Client,
    pub download_client: reqwest::Client,
    inner: Arc<Mutex<InnerState>>,
    pub processes: Arc<Mutex<HashMap<ComponentId, Child>>>,
    locks: Arc<HashMap<ComponentId, Arc<AsyncMutex<()>>>>,
}

struct InnerState {
    components: HashMap<ComponentId, RuntimeState>,
    logs: VecDeque<LogEntry>,
    last_update_check: Option<String>,
    provider_model_sync_last_success: Option<String>,
    provider_model_sync_last_error: Option<String>,
}

impl AppState {
    pub fn new(manager_config_dir: PathBuf, settings: ManagerSettings) -> AppResult<Self> {
        ensure_data_directories(&settings.data_directory)?;
        let components = load_runtime_components(&settings.data_directory);
        let mut locks = HashMap::new();
        for id in ComponentId::ALL {
            locks.insert(id, Arc::new(AsyncMutex::new(())));
        }
        let client = reqwest::Client::builder()
            .user_agent("CPA-Manager-Native/0.1")
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        let download_client = reqwest::Client::builder()
            .user_agent("CPA-Manager-Native/0.1")
            .build()?;

        let state = Self {
            manager_config_dir: Arc::new(manager_config_dir),
            settings: Arc::new(Mutex::new(settings)),
            client,
            download_client,
            inner: Arc::new(Mutex::new(InnerState {
                components,
                logs: VecDeque::new(),
                last_update_check: None,
                provider_model_sync_last_success: None,
                provider_model_sync_last_error: None,
            })),
            processes: Arc::new(Mutex::new(HashMap::new())),
            locks: Arc::new(locks),
        };
        state.log(LogSource::App, LogLevel::Info, "应用状态已初始化");
        Ok(state)
    }

    pub fn manager_config_dir(&self) -> &Path {
        self.manager_config_dir.as_ref()
    }

    pub fn root(&self) -> PathBuf {
        self.settings
            .lock()
            .expect("settings lock poisoned")
            .data_directory
            .clone()
    }

    pub fn launch_at_startup(&self) -> bool {
        self.settings
            .lock()
            .expect("settings lock poisoned")
            .launch_at_startup
    }

    pub fn lock_for(&self, id: ComponentId) -> Arc<AsyncMutex<()>> {
        self.locks.get(&id).expect("known component").clone()
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let settings = self
            .settings
            .lock()
            .expect("settings lock poisoned")
            .clone();
        let inner = self.inner.lock().expect("state lock poisoned");
        let components = COMPONENTS
            .iter()
            .map(|definition| {
                let runtime = inner
                    .components
                    .get(&definition.id)
                    .expect("known component");
                let installed_version = runtime.installed.as_ref().map(|item| item.version.clone());
                let port = settings.component_port(definition.id);
                ComponentSnapshot {
                    id: definition.id,
                    name: definition.name.to_string(),
                    short_name: definition.short_name.to_string(),
                    description: definition.description.to_string(),
                    repository: definition.repository.to_string(),
                    installed_version: installed_version.clone(),
                    latest_version: runtime.latest_version.clone(),
                    lifecycle: runtime.lifecycle,
                    healthy: runtime.healthy,
                    pid: runtime.pid,
                    port,
                    auto_start: settings.component_auto_start(definition.id),
                    management_url: Some(format!(
                        "http://127.0.0.1:{}{}",
                        port, definition.management_path
                    )),
                    management_key: runtime.management_key.clone(),
                    update_available: is_update_available(
                        installed_version.as_deref(),
                        runtime.latest_version.as_deref(),
                    ),
                    busy: runtime.busy,
                    progress_percent: runtime.progress_percent,
                    progress_label: runtime.progress_label.clone(),
                    last_error: runtime.last_error.clone(),
                }
            })
            .collect();
        AppSnapshot {
            platform: platform_name().to_string(),
            architecture: std::env::consts::ARCH.to_string(),
            manager_config_directory: self.manager_config_dir.display().to_string(),
            data_directory: settings.data_directory.display().to_string(),
            github_release_download_proxy: settings.github_release_download_proxy.clone(),
            launch_at_startup: settings.launch_at_startup,
            lan_access_enabled: cliproxy_lan_access_enabled(
                &settings
                    .data_directory
                    .join("data")
                    .join(ComponentId::Cliproxyapi.directory_name()),
            ),
            webdav: settings.webdav.clone(),
            provider_model_sync: settings.provider_model_sync.clone(),
            provider_model_sync_last_success: inner.provider_model_sync_last_success.clone(),
            provider_model_sync_last_error: inner.provider_model_sync_last_error.clone(),
            last_update_check: inner.last_update_check.clone(),
            components,
            logs: inner.logs.iter().cloned().collect(),
        }
    }

    pub fn emit_snapshot(&self, app: &AppHandle) {
        let _ = app.emit("app://snapshot", self.snapshot());
    }

    pub fn log(&self, source: LogSource, level: LogLevel, message: impl Into<String>) {
        let timestamp = Utc::now().to_rfc3339();
        let message = message.into();
        let mut inner = self.inner.lock().expect("state lock poisoned");
        inner.logs.push_back(LogEntry {
            timestamp: timestamp.clone(),
            component_id: source,
            level,
            message: message.clone(),
        });
        while inner.logs.len() > 500 {
            inner.logs.pop_front();
        }
        drop(inner);

        let source = serde_json::to_string(&source).unwrap_or_else(|_| "\"app\"".into());
        let level = serde_json::to_string(&level).unwrap_or_else(|_| "\"info\"".into());
        let logs_dir = self.logs_dir();
        let _ = fs::create_dir_all(&logs_dir);
        if let Ok(mut file) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(logs_dir.join("manager.log"))
        {
            let _ = writeln!(
                file,
                "{} [{}] [{}] {}",
                timestamp,
                source.trim_matches('"'),
                level.trim_matches('"'),
                message
            );
        }
    }

    pub fn with_component<R>(&self, id: ComponentId, action: impl FnOnce(&RuntimeState) -> R) -> R {
        let inner = self.inner.lock().expect("state lock poisoned");
        action(inner.components.get(&id).expect("known component"))
    }

    pub fn update_component(&self, id: ComponentId, action: impl FnOnce(&mut RuntimeState)) {
        let mut inner = self.inner.lock().expect("state lock poisoned");
        action(inner.components.get_mut(&id).expect("known component"));
    }

    pub fn set_latest_version(&self, id: ComponentId, version: String) {
        self.update_component(id, |runtime| runtime.latest_version = Some(version));
    }

    pub fn mark_update_check(&self) {
        self.inner
            .lock()
            .expect("state lock poisoned")
            .last_update_check = Some(Utc::now().to_rfc3339());
    }

    pub fn record_provider_model_sync_success(&self) {
        let mut inner = self.inner.lock().expect("state lock poisoned");
        inner.provider_model_sync_last_success = Some(Utc::now().to_rfc3339());
        inner.provider_model_sync_last_error = None;
    }

    pub fn record_provider_model_sync_error(&self, error: String) {
        self.inner
            .lock()
            .expect("state lock poisoned")
            .provider_model_sync_last_error = Some(error);
    }

    pub fn set_launch_at_startup(&self, enabled: bool) -> AppResult<()> {
        let mut next = self
            .settings
            .lock()
            .expect("settings lock poisoned")
            .clone();
        next.launch_at_startup = enabled;
        write_manager_settings(&self.manager_config_dir, &next)?;
        *self.settings.lock().expect("settings lock poisoned") = next;
        Ok(())
    }

    pub fn component_port(&self, id: ComponentId) -> u16 {
        self.settings
            .lock()
            .expect("settings lock poisoned")
            .component_port(id)
    }

    pub fn component_auto_start(&self, id: ComponentId) -> bool {
        self.settings
            .lock()
            .expect("settings lock poisoned")
            .component_auto_start(id)
    }

    pub fn set_component_auto_start(&self, id: ComponentId, enabled: bool) -> AppResult<()> {
        let mut next = self.settings();
        next.auto_start_components.insert(id, enabled);
        self.replace_settings(next)
    }

    pub fn set_component_port(&self, id: ComponentId, port: u16) -> AppResult<()> {
        let mut next = self.settings();
        next.component_ports.insert(id, port);
        self.replace_settings(next)
    }

    pub fn settings(&self) -> ManagerSettings {
        self.settings
            .lock()
            .expect("settings lock poisoned")
            .clone()
    }

    pub fn replace_settings(&self, settings: ManagerSettings) -> AppResult<()> {
        write_manager_settings(&self.manager_config_dir, &settings)?;
        *self.settings.lock().expect("settings lock poisoned") = settings;
        Ok(())
    }

    pub fn switch_data_directory(&self, new_root: PathBuf) -> AppResult<()> {
        ensure_data_directories(&new_root)?;
        {
            let mut next = self
                .settings
                .lock()
                .expect("settings lock poisoned")
                .clone();
            next.data_directory = new_root.clone();
            write_manager_settings(&self.manager_config_dir, &next)?;
            *self.settings.lock().expect("settings lock poisoned") = next;
        }
        let mut components = load_runtime_components(&new_root);
        let mut inner = self.inner.lock().expect("state lock poisoned");
        for id in ComponentId::ALL {
            let latest_version = inner
                .components
                .get(&id)
                .and_then(|runtime| runtime.latest_version.clone());
            if let (Some(runtime), Some(latest_version)) = (components.get_mut(&id), latest_version)
            {
                runtime.latest_version = Some(latest_version);
            }
        }
        inner.components = components;
        Ok(())
    }

    pub fn current_manifest_path(&self, id: ComponentId) -> PathBuf {
        component_root(&self.root(), id).join("current.json")
    }

    pub fn component_data_dir(&self, id: ComponentId) -> PathBuf {
        self.root().join("data").join(id.directory_name())
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root().join("logs")
    }
}

fn ensure_data_directories(root: &Path) -> AppResult<()> {
    fs::create_dir_all(root.join("components"))?;
    fs::create_dir_all(root.join("data"))?;
    fs::create_dir_all(root.join("logs"))?;
    fs::create_dir_all(root.join("downloads"))?;
    Ok(())
}

fn load_runtime_components(root: &Path) -> HashMap<ComponentId, RuntimeState> {
    let mut components = HashMap::new();
    for id in ComponentId::ALL {
        let manifest = load_manifest(root, id).ok();
        let mut runtime = RuntimeState::new(manifest);
        if id == ComponentId::Cliproxyapi {
            runtime.management_key =
                read_cliproxy_secret(&root.join("data").join(id.directory_name()));
        }
        components.insert(id, runtime);
    }
    components
}

pub fn component_root(root: &Path, id: ComponentId) -> PathBuf {
    root.join("components").join(id.directory_name())
}

pub fn load_manifest(root: &Path, id: ComponentId) -> AppResult<InstallManifest> {
    let data = fs::read(component_root(root, id).join("current.json"))?;
    Ok(serde_json::from_slice(&data)?)
}

pub fn write_manifest(path: &Path, manifest: &InstallManifest) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(manifest)?)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn platform_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        value => value,
    }
}

pub fn set_error(state: &AppState, id: ComponentId, error: &str) {
    state.update_component(id, |runtime| {
        runtime.lifecycle = LifecycleState::Error;
        runtime.healthy = false;
        runtime.busy = false;
        runtime.progress_percent = None;
        runtime.progress_label = None;
        runtime.last_error = Some(error.to_string());
    });
    state.log(LogSource::from(id), LogLevel::Error, error);
}

pub fn component_is_installed(state: &AppState, id: ComponentId) -> bool {
    state.with_component(id, |runtime| runtime.installed.is_some())
}
