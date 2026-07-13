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
    config::read_cliproxy_secret,
    error::AppResult,
    github::is_update_available,
    models::{
        AppSnapshot, ComponentId, ComponentSnapshot, InstallManifest, LifecycleState, LogEntry,
        LogLevel, LogSource, RuntimeState,
    },
};

#[derive(Clone)]
pub struct AppState {
    root: Arc<PathBuf>,
    pub client: reqwest::Client,
    inner: Arc<Mutex<InnerState>>,
    pub processes: Arc<Mutex<HashMap<ComponentId, Child>>>,
    locks: Arc<HashMap<ComponentId, Arc<AsyncMutex<()>>>>,
}

struct InnerState {
    components: HashMap<ComponentId, RuntimeState>,
    logs: VecDeque<LogEntry>,
    last_update_check: Option<String>,
}

impl AppState {
    pub fn new(root: PathBuf) -> AppResult<Self> {
        fs::create_dir_all(root.join("components"))?;
        fs::create_dir_all(root.join("data"))?;
        fs::create_dir_all(root.join("logs"))?;
        fs::create_dir_all(root.join("downloads"))?;

        let mut components = HashMap::new();
        let mut locks = HashMap::new();
        for id in ComponentId::ALL {
            let manifest = load_manifest(&root, id).ok();
            let mut runtime = RuntimeState::new(manifest);
            if id == ComponentId::Cliproxyapi {
                runtime.management_key =
                    read_cliproxy_secret(&root.join("data").join(id.directory_name()));
            }
            components.insert(id, runtime);
            locks.insert(id, Arc::new(AsyncMutex::new(())));
        }

        let client = reqwest::Client::builder()
            .user_agent("CPA-Manager-Native/0.1")
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        let state = Self {
            root: Arc::new(root),
            client,
            inner: Arc::new(Mutex::new(InnerState {
                components,
                logs: VecDeque::new(),
                last_update_check: None,
            })),
            processes: Arc::new(Mutex::new(HashMap::new())),
            locks: Arc::new(locks),
        };
        state.log(LogSource::App, LogLevel::Info, "应用状态已初始化");
        Ok(state)
    }

    pub fn root(&self) -> &Path {
        self.root.as_ref()
    }

    pub fn lock_for(&self, id: ComponentId) -> Arc<AsyncMutex<()>> {
        self.locks.get(&id).expect("known component").clone()
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let inner = self.inner.lock().expect("state lock poisoned");
        let components = COMPONENTS
            .iter()
            .map(|definition| {
                let runtime = inner
                    .components
                    .get(&definition.id)
                    .expect("known component");
                let installed_version = runtime.installed.as_ref().map(|item| item.version.clone());
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
                    port: definition.port,
                    management_url: Some(format!(
                        "http://127.0.0.1:{}{}",
                        definition.port, definition.management_path
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
            data_directory: self.root.display().to_string(),
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
        if let Ok(mut file) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.logs_dir().join("manager.log"))
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

    pub fn current_manifest_path(&self, id: ComponentId) -> PathBuf {
        component_root(self.root(), id).join("current.json")
    }

    pub fn component_data_dir(&self, id: ComponentId) -> PathBuf {
        self.root.join("data").join(id.directory_name())
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
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
