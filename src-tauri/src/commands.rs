use tauri::{AppHandle, State};

use crate::{
    error::{message, AppResult},
    github::latest_release,
    installer,
    models::{AppSnapshot, ComponentId, LifecycleState, LogLevel, LogSource},
    process,
    state::{component_is_installed, set_error, AppState},
};

#[tauri::command]
pub fn get_app_snapshot(state: State<'_, AppState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
pub async fn check_updates(app: AppHandle, state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    let state = state.inner().clone();
    let (core, manager) = futures_util::future::join(
        latest_release(&state.client, ComponentId::Cliproxyapi),
        latest_release(&state.client, ComponentId::CpaManagerPlus),
    )
    .await;
    let mut errors = Vec::new();
    for (id, result) in [
        (ComponentId::Cliproxyapi, core),
        (ComponentId::CpaManagerPlus, manager),
    ] {
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
    for id in [ComponentId::CpaManagerPlus, ComponentId::Cliproxyapi] {
        let running = state.with_component(id, |runtime| runtime.pid.is_some());
        if running {
            stop_one(&app, &state, id).await?;
        }
    }
    Ok(state.snapshot())
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
        definition.port, definition.management_path
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
        if component_is_installed(state, id) {
            let _ = start_one(app, state, id).await;
        }
    }
}

fn should_rollback_after_start_error(error: &str) -> bool {
    error.contains("无法启动组件")
        || error.contains("进程在健康检查完成前退出")
        || error.contains("启动超时")
}

pub(crate) async fn stop_managed_components(state: &AppState) {
    for id in [ComponentId::CpaManagerPlus, ComponentId::Cliproxyapi] {
        let running = state.with_component(id, |runtime| runtime.pid.is_some());
        if running {
            let _ = process::stop(state, id).await;
        }
    }
}
