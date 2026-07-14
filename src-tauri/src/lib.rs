mod commands;
mod components;
mod config;
mod error;
mod github;
mod installer;
mod models;
mod process;
mod state;

use std::time::Duration;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, RunEvent, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;

use crate::{
    models::{ComponentId, LogLevel, LogSource},
    state::AppState,
};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let default_data_dir = app.path().app_data_dir()?;
            let config_dir = config::manager_config_dir()?;
            let settings = config::load_manager_settings(&config_dir, &default_data_dir)?;
            let state = AppState::new(config_dir, settings)?;
            app.manage(state.clone());
            sync_launch_at_startup(app.handle(), &state);

            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
            let mut tray = TrayIconBuilder::with_id("main-tray")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("CPA Manager Native")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<AppState>().inner().clone();
                commands::start_installed_components(&app_handle, &state).await;
                loop {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    let state = app_handle.state::<AppState>().inner().clone();
                    for id in ComponentId::ALL {
                        let _ = process::refresh_health(&state, id).await;
                    }
                    state.emit_snapshot(&app_handle);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_snapshot,
            commands::check_updates,
            commands::install_component,
            commands::start_component,
            commands::stop_component,
            commands::start_all,
            commands::stop_all,
            commands::select_data_directory,
            commands::change_data_directory,
            commands::set_launch_at_startup,
            commands::open_management_page,
            commands::open_log_directory,
            commands::open_repository,
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build CPA Manager Native")
        .run(|app, event| {
            if matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
                let state = app.state::<AppState>().inner().clone();
                tauri::async_runtime::block_on(async {
                    commands::stop_managed_components(&state).await;
                });
            }
        });
}

fn sync_launch_at_startup(app: &AppHandle, state: &AppState) {
    let desired = state.launch_at_startup();
    let result = if desired {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    };
    if let Err(error) = result {
        state.log(
            LogSource::App,
            LogLevel::Warn,
            format!("同步开机自启状态失败：{error}"),
        );
        return;
    }

    let actual = app.autolaunch().is_enabled().unwrap_or(desired);
    if let Err(error) = state.set_launch_at_startup(actual) {
        state.log(
            LogSource::App,
            LogLevel::Warn,
            format!("保存开机自启状态失败：{error}"),
        );
    }
}
