use std::{
    fs,
    path::{Path, PathBuf},
};

use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use crate::{
    components::locate_executable,
    config::{set_cliproxy_lan_access, WebDavSettings},
    error::{message, AppResult},
    github::latest_release,
    installer,
    models::{AppSnapshot, ComponentId, InstallManifest, LifecycleState, LogLevel, LogSource},
    process,
    state::{component_is_installed, component_root, set_error, write_manifest, AppState},
    webdav,
};

use crate::config::ProviderModelSyncSettings;

#[tauri::command]
pub fn get_app_snapshot(state: State<'_, AppState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
pub async fn check_updates(app: AppHandle, state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    let releases = futures_util::future::join_all(ComponentId::ALL.into_iter().map(|id| {
        let state = &state;
        async move { (id, latest_release(&state.client, id).await) }
    }))
    .await;
    let mut errors = Vec::new();
    for (id, result) in releases {
        match result {
            Ok(release) => state.set_latest_version(id, release.tag_name),
            Err(error) => errors.push(error.to_string()),
        }
    }
    state.mark_update_check();
    if errors.is_empty() {
        state.log(LogSource::App, LogLevel::Info, "GitHub Release 检查完成");
    } else {
        state.log(LogSource::App, LogLevel::Warn, errors.join("；"));
    }
    state.emit_snapshot(&app);
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn install_component(
    app: AppHandle,
    state: State<'_, AppState>,
    component_id: ComponentId,
) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    let lock = state.lock_for(component_id);
    let _guard = lock.lock().await;
    if let Err(error) = installer::install(&app, &state, component_id).await {
        set_error(&state, component_id, &error.to_string());
        state.emit_snapshot(&app);
        return Err(error);
    }
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn start_component(
    app: AppHandle,
    state: State<'_, AppState>,
    component_id: ComponentId,
) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    start_one(&app, &state, component_id).await?;
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn stop_component(
    app: AppHandle,
    state: State<'_, AppState>,
    component_id: ComponentId,
) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    stop_one(&app, &state, component_id).await?;
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn sync_provider_models_now(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    match crate::provider_model_sync::synchronize(&state).await {
        Ok(changed) => {
            state.record_provider_model_sync_success();
            if changed {
                restart_cliproxyapi(&app, &state).await?;
            }
            state.emit_snapshot(&app);
            Ok(state.snapshot())
        }
        Err(error) => {
            state.record_provider_model_sync_error(error.to_string());
            state.emit_snapshot(&app);
            Err(error)
        }
    }
}

#[tauri::command]
pub fn save_provider_model_sync_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: ProviderModelSyncSettings,
) -> AppResult<AppSnapshot> {
    crate::provider_model_sync::validate_settings(&settings)?;
    let mut manager_settings = state.settings();
    manager_settings.provider_model_sync = settings;
    state.replace_settings(manager_settings)?;
    state.log(
        LogSource::App,
        LogLevel::Info,
        "Provider 模型同步设置已保存",
    );
    state.emit_snapshot(&app);
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn start_all(app: AppHandle, state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    for id in ComponentId::ALL {
        if !component_is_installed(&state, id) {
            let lock = state.lock_for(id);
            let _guard = lock.lock().await;
            if let Err(error) = installer::install(&app, &state, id).await {
                set_error(&state, id, &error.to_string());
                return Err(error);
            }
        }
        start_one(&app, &state, id).await?;
    }
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn stop_all(app: AppHandle, state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    for id in ComponentId::ALL.into_iter().rev() {
        let running = state.with_component(id, |runtime| runtime.pid.is_some());
        if running {
            stop_one(&app, &state, id).await?;
        }
    }
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn select_data_directory(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<String>> {
    let current = state.root();
    let (sender, receiver) = oneshot::channel();
    app.dialog()
        .file()
        .set_title("选择 CPA 应用数据目录")
        .set_directory(current)
        .set_can_create_directories(true)
        .pick_folder(move |folder| {
            let _ = sender.send(folder);
        });

    let selected = receiver.await.map_err(|_| message("目录选择窗口已关闭"))?;
    let Some(path) = selected else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| message(format!("无法读取目录路径：{error}")))?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub async fn change_data_directory(
    app: AppHandle,
    state: State<'_, AppState>,
    directory: String,
) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    if let Some(id) = ComponentId::ALL
        .into_iter()
        .find(|id| state.with_component(*id, |runtime| runtime.busy))
    {
        return Err(message(format!(
            "{id} 正在执行任务，请完成后再迁移数据目录"
        )));
    }

    let (old_root, new_root) = prepare_migration_paths(&state, &directory)?;
    let running_before: Vec<ComponentId> = ComponentId::ALL
        .into_iter()
        .filter(|id| state.with_component(*id, |runtime| runtime.pid.is_some()))
        .collect();
    for id in ComponentId::ALL.into_iter().rev() {
        if running_before.contains(&id) {
            stop_one(&app, &state, id).await?;
        }
    }

    state.log(
        LogSource::App,
        LogLevel::Info,
        format!(
            "开始迁移应用数据目录：{} -> {}",
            old_root.display(),
            new_root.display()
        ),
    );

    let migration = tokio::task::spawn_blocking({
        let old_root = old_root.clone();
        let new_root = new_root.clone();
        move || migrate_data_directory(&old_root, &new_root)
    })
    .await
    .map_err(|error| message(format!("迁移任务异常结束：{error}")))
    .and_then(|result| result);

    if let Err(error) = migration {
        state.log(
            LogSource::App,
            LogLevel::Error,
            format!("迁移应用数据目录失败：{error}"),
        );
        restart_previously_running(&app, &state, &running_before).await;
        state.emit_snapshot(&app);
        return Err(error);
    }

    if let Err(error) = state.switch_data_directory(new_root.clone()) {
        state.log(
            LogSource::App,
            LogLevel::Error,
            format!("切换应用数据目录失败：{error}"),
        );
        restart_previously_running(&app, &state, &running_before).await;
        state.emit_snapshot(&app);
        return Err(error);
    }
    state.log(
        LogSource::App,
        LogLevel::Info,
        format!(
            "应用数据目录已切换到 {}；原目录保留为回滚备份",
            new_root.display()
        ),
    );
    restart_previously_running(&app, &state, &running_before).await;
    state.emit_snapshot(&app);
    Ok(state.snapshot())
}

#[tauri::command]
pub fn set_launch_at_startup(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<AppSnapshot> {
    let autolaunch = app.autolaunch();
    let result = if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    result.map_err(|error| message(format!("无法更新开机自启：{error}")))?;
    let actual = autolaunch.is_enabled().unwrap_or(enabled);
    state.set_launch_at_startup(actual)?;
    state.log(
        LogSource::App,
        LogLevel::Info,
        if actual {
            "开机自启已启用"
        } else {
            "开机自启已关闭"
        },
    );
    state.emit_snapshot(&app);
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn set_lan_access(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    let id = ComponentId::Cliproxyapi;
    let lock = state.lock_for(id);
    let _guard = lock.lock().await;
    let was_running = state.with_component(id, |runtime| runtime.pid.is_some());

    if was_running {
        process::stop(&state, id).await?;
    }

    let data_dir = state.component_data_dir(id);
    if let Err(config_error) = set_cliproxy_lan_access(&data_dir, enabled) {
        let mut error_message = format!("无法更新局域网访问设置：{config_error}");
        if was_running {
            if let Err(restart_error) = process::start(&app, &state, id).await {
                error_message.push_str(&format!("；恢复运行失败：{restart_error}"));
                set_error(&state, id, &error_message);
            }
        }
        state.log(LogSource::App, LogLevel::Error, &error_message);
        state.emit_snapshot(&app);
        return Err(message(error_message));
    }

    state.log(
        LogSource::App,
        LogLevel::Info,
        if enabled {
            "CLIProxyAPI 局域网访问已开启（host: 0.0.0.0，allow-remote: true）"
        } else {
            "CLIProxyAPI 局域网访问已关闭（host: 127.0.0.1，allow-remote: false）"
        },
    );

    if was_running {
        if let Err(error) = process::start(&app, &state, id).await {
            let error_message = format!("局域网访问设置已保存，但 CLIProxyAPI 重启失败：{error}");
            set_error(&state, id, &error_message);
            state.emit_snapshot(&app);
            return Err(message(error_message));
        }
    }

    state.emit_snapshot(&app);
    Ok(state.snapshot())
}

#[tauri::command]
pub fn set_component_auto_start(
    app: AppHandle,
    state: State<'_, AppState>,
    component_id: ComponentId,
    enabled: bool,
) -> AppResult<AppSnapshot> {
    state.set_component_auto_start(component_id, enabled)?;
    state.log(
        LogSource::from(component_id),
        LogLevel::Info,
        if enabled {
            "已设置为随应用自动启动"
        } else {
            "已关闭随应用自动启动"
        },
    );
    state.emit_snapshot(&app);
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn set_component_port(
    app: AppHandle,
    state: State<'_, AppState>,
    component_id: ComponentId,
    port: u16,
) -> AppResult<AppSnapshot> {
    if port == 0 {
        return Err(message("端口必须在 1 到 65535 之间"));
    }
    if let Some(conflict) = ComponentId::ALL
        .into_iter()
        .find(|id| *id != component_id && state.component_port(*id) == port)
    {
        return Err(message(format!(
            "端口 {port} 已分配给 {}",
            crate::components::definition(conflict).name
        )));
    }

    let state = state.inner().clone();
    let lock = state.lock_for(component_id);
    let _guard = lock.lock().await;
    let dependent_lock = (component_id == ComponentId::Cliproxyapi)
        .then(|| state.lock_for(ComponentId::CpaManagerPlus));
    let _dependent_guard = if let Some(lock) = dependent_lock.as_ref() {
        Some(lock.lock().await)
    } else {
        None
    };
    if state.with_component(component_id, |runtime| runtime.busy) {
        return Err(message("组件正在执行任务，请稍后再修改端口"));
    }
    if component_id == ComponentId::Cliproxyapi
        && state.with_component(ComponentId::CpaManagerPlus, |runtime| runtime.busy)
    {
        return Err(message(
            "CPA-Manager-Plus 正在执行任务，请稍后再修改 CLIProxyAPI 端口",
        ));
    }
    let old_port = state.component_port(component_id);
    if old_port == port {
        return Ok(state.snapshot());
    }
    let was_running = state.with_component(component_id, |runtime| runtime.pid.is_some());
    let manager_was_running = component_id == ComponentId::Cliproxyapi
        && state.with_component(ComponentId::CpaManagerPlus, |runtime| runtime.pid.is_some());
    if manager_was_running {
        process::stop(&state, ComponentId::CpaManagerPlus).await?;
    }
    if was_running {
        process::stop(&state, component_id).await?;
    }
    state.set_component_port(component_id, port)?;
    state.log(
        LogSource::from(component_id),
        LogLevel::Info,
        format!("监听端口已从 {old_port} 修改为 {port}"),
    );
    if was_running {
        if let Err(error) = process::start(&app, &state, component_id).await {
            let error_message = format!("端口设置已保存，但组件重启失败：{error}");
            set_error(&state, component_id, &error_message);
            state.emit_snapshot(&app);
            return Err(message(error_message));
        }
    }
    if manager_was_running {
        if let Err(error) = process::start(&app, &state, ComponentId::CpaManagerPlus).await {
            let error_message =
                format!("CLIProxyAPI 端口设置已生效，但 CPA-Manager-Plus 联动重启失败：{error}");
            set_error(&state, ComponentId::CpaManagerPlus, &error_message);
            state.emit_snapshot(&app);
            return Err(message(error_message));
        }
    }
    state.emit_snapshot(&app);
    Ok(state.snapshot())
}

#[tauri::command]
pub fn save_webdav_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: WebDavSettings,
) -> AppResult<AppSnapshot> {
    let mut next = state.settings();
    next.webdav = settings;
    state.replace_settings(next)?;
    state.log(LogSource::App, LogLevel::Info, "WebDAV 同步设置已保存");
    state.emit_snapshot(&app);
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn test_webdav_connection(
    state: State<'_, AppState>,
    settings: WebDavSettings,
) -> AppResult<()> {
    webdav::test(state.inner(), &settings).await
}

#[tauri::command]
pub async fn upload_webdav_config(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    match webdav::upload(&state).await {
        Ok(()) => {
            update_webdav_status(&state, None)?;
            state.log(
                LogSource::App,
                LogLevel::Info,
                "WebDAV 配置上传完成（3 个配置文件白名单）",
            );
        }
        Err(error) => {
            update_webdav_status(&state, Some(error.to_string()))?;
            state.log(
                LogSource::App,
                LogLevel::Error,
                format!("WebDAV 配置上传失败：{error}"),
            );
            state.emit_snapshot(&app);
            return Err(error);
        }
    }
    state.emit_snapshot(&app);
    Ok(state.snapshot())
}

#[tauri::command]
pub async fn download_webdav_config(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    if ComponentId::ALL
        .into_iter()
        .any(|id| state.with_component(id, |runtime| runtime.pid.is_some() || runtime.busy))
    {
        return Err(message(
            "下载配置前请先停止所有组件，并等待安装或更新任务完成",
        ));
    }
    match webdav::download(&state).await {
        Ok(()) => {
            update_webdav_status(&state, None)?;
            state.log(
                LogSource::App,
                LogLevel::Info,
                "WebDAV 配置下载并恢复完成，本地原文件已备份",
            );
        }
        Err(error) => {
            update_webdav_status(&state, Some(error.to_string()))?;
            state.log(
                LogSource::App,
                LogLevel::Error,
                format!("WebDAV 配置下载失败：{error}"),
            );
            state.emit_snapshot(&app);
            return Err(error);
        }
    }
    state.emit_snapshot(&app);
    Ok(state.snapshot())
}

fn update_webdav_status(state: &AppState, error: Option<String>) -> AppResult<()> {
    let mut next = state.settings();
    if error.is_none() {
        next.webdav.last_sync_at = Some(chrono::Utc::now().to_rfc3339());
    }
    next.webdav.last_error = error;
    state.replace_settings(next)
}

#[tauri::command]
pub fn open_management_page(
    state: State<'_, AppState>,
    component_id: ComponentId,
) -> AppResult<AppSnapshot> {
    let definition = crate::components::definition(component_id);
    let healthy = state.with_component(component_id, |runtime| runtime.healthy);
    if !healthy {
        return Err(message("组件尚未通过健康检查"));
    }
    let url = format!(
        "http://127.0.0.1:{}{}",
        state.component_port(component_id),
        definition.management_path
    );
    open::that(url).map_err(|error| message(format!("无法打开管理页面：{error}")))?;
    Ok(state.snapshot())
}

#[tauri::command]
pub fn open_log_directory(state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    open::that(state.logs_dir()).map_err(|error| message(format!("无法打开日志目录：{error}")))?;
    Ok(state.snapshot())
}

#[tauri::command]
pub fn open_repository(
    state: State<'_, AppState>,
    component_id: ComponentId,
) -> AppResult<AppSnapshot> {
    let repository = crate::components::definition(component_id).repository;
    open::that(format!("https://github.com/{repository}"))
        .map_err(|error| message(format!("无法打开 GitHub 仓库：{error}")))?;
    Ok(state.snapshot())
}

async fn start_one(app: &AppHandle, state: &AppState, id: ComponentId) -> AppResult<()> {
    let lock = state.lock_for(id);
    let _guard = lock.lock().await;
    let running = state.with_component(id, |runtime| runtime.lifecycle == LifecycleState::Running);
    if running {
        return Ok(());
    }
    if let Err(error) = process::start(app, state, id).await {
        let original_error = error.to_string();
        if should_rollback_after_start_error(&original_error) {
            match installer::rollback_to_previous_version(state, id) {
                Ok(Some(previous)) => {
                    state.log(
                        LogSource::from(id),
                        LogLevel::Warn,
                        format!(
                            "启动 {} 失败，尝试使用上一版本 {} 重启：{}",
                            id, previous.version, original_error
                        ),
                    );
                    state.emit_snapshot(app);
                    if let Err(retry_error) = process::start(app, state, id).await {
                        let combined = format!(
                            "启动失败；已尝试回退到 {} 但仍失败：{}（原错误：{}）",
                            previous.version, retry_error, original_error
                        );
                        set_error(state, id, &combined);
                        state.emit_snapshot(app);
                        return Err(message(combined));
                    }
                    return Ok(());
                }
                Ok(None) => {}
                Err(rollback_error) => {
                    state.log(
                        LogSource::from(id),
                        LogLevel::Warn,
                        format!("启动失败且无法回退到上一版本：{rollback_error}"),
                    );
                }
            }
        }
        set_error(state, id, &error.to_string());
        state.emit_snapshot(app);
        return Err(error);
    }
    Ok(())
}

async fn stop_one(app: &AppHandle, state: &AppState, id: ComponentId) -> AppResult<()> {
    let lock = state.lock_for(id);
    let _guard = lock.lock().await;
    process::stop(state, id).await?;
    state.emit_snapshot(app);
    Ok(())
}

pub(crate) async fn start_installed_components(app: &AppHandle, state: &AppState) {
    for id in ComponentId::ALL {
        if component_is_installed(state, id) && state.component_auto_start(id) {
            let _ = start_one(app, state, id).await;
        }
    }
}

pub(crate) async fn restart_cliproxyapi(app: &AppHandle, state: &AppState) -> AppResult<()> {
    let id = ComponentId::Cliproxyapi;
    let lock = state.lock_for(id);
    let _guard = lock.lock().await;
    let was_running = state.with_component(id, |runtime| runtime.pid.is_some());
    if !was_running {
        return Ok(());
    }
    process::stop(state, id).await?;
    process::start(app, state, id).await?;
    state.emit_snapshot(app);
    Ok(())
}

fn should_rollback_after_start_error(error: &str) -> bool {
    error.contains("无法启动组件")
        || error.contains("进程在健康检查完成前退出")
        || error.contains("启动超时")
}

pub(crate) async fn stop_managed_components(state: &AppState) {
    for id in ComponentId::ALL.into_iter().rev() {
        let running = state.with_component(id, |runtime| runtime.pid.is_some());
        if running {
            let _ = process::stop(state, id).await;
        }
    }
}

async fn restart_previously_running(app: &AppHandle, state: &AppState, running: &[ComponentId]) {
    for id in ComponentId::ALL {
        if running.contains(&id) && component_is_installed(state, id) {
            let _ = start_one(app, state, id).await;
        }
    }
}

fn prepare_migration_paths(state: &AppState, directory: &str) -> AppResult<(PathBuf, PathBuf)> {
    let old_root = state.root();
    let new_root = PathBuf::from(directory.trim());
    if new_root.as_os_str().is_empty() {
        return Err(message("请选择新的应用数据目录"));
    }
    if !new_root.is_absolute() {
        return Err(message("应用数据目录必须是绝对路径"));
    }

    fs::create_dir_all(&old_root)?;
    fs::create_dir_all(&new_root)?;
    let old_canonical = fs::canonicalize(&old_root)?;
    let new_canonical = fs::canonicalize(&new_root)?;
    if old_canonical == new_canonical {
        return Err(message("新目录与当前应用数据目录相同"));
    }
    if new_canonical.starts_with(&old_canonical) || old_canonical.starts_with(&new_canonical) {
        return Err(message("新旧应用数据目录不能互相包含"));
    }

    let config_canonical = fs::canonicalize(state.manager_config_dir())?;
    if new_canonical == config_canonical
        || new_canonical.starts_with(&config_canonical)
        || config_canonical.starts_with(&new_canonical)
    {
        return Err(message("应用数据目录不能与固定配置目录互相包含"));
    }

    Ok((old_root, new_root))
}

fn migrate_data_directory(old_root: &Path, new_root: &Path) -> AppResult<()> {
    copy_directory_contents(old_root, new_root)?;
    rewrite_migrated_manifests(old_root, new_root)
}

fn copy_directory_contents(source: &Path, destination: &Path) -> AppResult<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_directory_contents(&source_path, &target_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &target_path)?;
        } else {
            return Err(message(format!(
                "数据目录包含暂不支持迁移的文件类型：{}",
                source_path.display()
            )));
        }
    }
    Ok(())
}

fn rewrite_migrated_manifests(old_root: &Path, new_root: &Path) -> AppResult<()> {
    for id in ComponentId::ALL {
        let manifest_path = component_root(new_root, id).join("current.json");
        if !manifest_path.exists() {
            continue;
        }
        let mut manifest: InstallManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        manifest.executable_path = migrated_executable_path(old_root, new_root, id, &manifest)?;
        write_manifest(&manifest_path, &manifest)?;
    }
    Ok(())
}

fn migrated_executable_path(
    old_root: &Path,
    new_root: &Path,
    id: ComponentId,
    manifest: &InstallManifest,
) -> AppResult<PathBuf> {
    if let Ok(relative) = manifest.executable_path.strip_prefix(old_root) {
        return Ok(new_root.join(relative));
    }

    let version_root = component_root(new_root, id)
        .join("versions")
        .join(manifest.version.trim_start_matches('v'));
    if version_root.exists() {
        if let Ok(path) = locate_executable(&version_root, id) {
            return Ok(path);
        }
    }
    locate_executable(&component_root(new_root, id), id)
}
