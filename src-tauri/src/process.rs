use std::{
    fs,
    io::{BufRead, BufReader},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    process::{Command, Stdio},
    time::Duration,
};

use tauri::AppHandle;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::{
    components::definition,
    config::ensure_cliproxy_secret,
    error::{message, AppResult},
    models::{ComponentId, LifecycleState, LogLevel, LogSource},
    state::AppState,
};

pub async fn start(app: &AppHandle, state: &AppState, id: ComponentId) -> AppResult<()> {
    let definition = definition(id);
    let installed = state
        .with_component(id, |runtime| runtime.installed.clone())
        .ok_or_else(|| message("组件尚未安装"))?;
    if state
        .processes
        .lock()
        .expect("process lock poisoned")
        .contains_key(&id)
    {
        return Err(message("组件已经在运行"));
    }
    prepare_component_data(state, id)?;
    ensure_port_available(definition.port)?;

    state.update_component(id, |runtime| {
        runtime.lifecycle = LifecycleState::Starting;
        runtime.busy = true;
        runtime.progress_label = Some("等待服务健康检查".into());
        runtime.progress_percent = None;
        runtime.last_error = None;
    });
    state.emit_snapshot(app);

    let data_dir = state.component_data_dir(id);
    let mut command = Command::new(&installed.executable_path);
    command
        .current_dir(&data_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match id {
        ComponentId::Cliproxyapi => {
            command.arg("--config").arg(data_dir.join("config.yaml"));
        }
        ComponentId::CpaManagerPlus => {
            command
                .env("CPA_MANAGER_CONFIG", data_dir.join("config.json"))
                .env("HTTP_ADDR", format!("127.0.0.1:{}", definition.port))
                .env("USAGE_DATA_DIR", data_dir.join("data"))
                .env("CPA_UPSTREAM_URL", "http://127.0.0.1:8317");
        }
    }
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
    }

    let mut child = command
        .spawn()
        .map_err(|error| message(format!("无法启动组件：{error}")))?;
    let pid = child.id();
    if let Some(stdout) = child.stdout.take() {
        capture_output(state.clone(), id, stdout, LogLevel::Info);
    }
    if let Some(stderr) = child.stderr.take() {
        capture_output(state.clone(), id, stderr, LogLevel::Error);
    }
    state
        .processes
        .lock()
        .expect("process lock poisoned")
        .insert(id, child);
    state.update_component(id, |runtime| runtime.pid = Some(pid));
    state.log(
        LogSource::from(id),
        LogLevel::Info,
        format!("进程已启动，PID {pid}"),
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        if process_exited(state, id)? {
            return Err(message("进程在健康检查完成前退出"));
        }
        if service_health(state, id).await {
            state.update_component(id, |runtime| {
                runtime.lifecycle = LifecycleState::Running;
                runtime.healthy = true;
                runtime.busy = false;
                runtime.progress_label = None;
                runtime.last_error = None;
            });
            state.log(LogSource::from(id), LogLevel::Info, "健康检查通过");
            state.emit_snapshot(app);
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = stop(state, id).await;
            return Err(message("启动超时：15 秒内未通过端口健康检查"));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub async fn stop(state: &AppState, id: ComponentId) -> AppResult<()> {
    state.update_component(id, |runtime| {
        runtime.lifecycle = LifecycleState::Stopping;
        runtime.busy = true;
        runtime.healthy = false;
    });
    let child = state
        .processes
        .lock()
        .expect("process lock poisoned")
        .remove(&id);
    if let Some(mut child) = child {
        #[cfg(windows)]
        {
            let status = Command::new("taskkill")
                .args(["/PID", &child.id().to_string(), "/T", "/F"])
                .creation_flags(0x08000000)
                .status();
            if status.is_err() {
                let _ = child.kill();
            }
        }
        #[cfg(not(windows))]
        {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
    state.update_component(id, |runtime| {
        runtime.lifecycle = if runtime.installed.is_some() {
            LifecycleState::Stopped
        } else {
            LifecycleState::NotInstalled
        };
        runtime.pid = None;
        runtime.healthy = false;
        runtime.busy = false;
        runtime.progress_label = None;
    });
    state.log(LogSource::from(id), LogLevel::Info, "组件已停止");
    Ok(())
}

pub async fn refresh_health(state: &AppState, id: ComponentId) -> AppResult<()> {
    if process_exited(state, id)? {
        return Ok(());
    }
    let healthy = service_health(state, id).await;
    state.update_component(id, |runtime| {
        if runtime.pid.is_some() {
            runtime.lifecycle = LifecycleState::Running;
            runtime.healthy = healthy;
        }
    });
    Ok(())
}

fn process_exited(state: &AppState, id: ComponentId) -> AppResult<bool> {
    let mut processes = state.processes.lock().expect("process lock poisoned");
    let Some(child) = processes.get_mut(&id) else {
        return Ok(false);
    };
    if let Some(status) = child.try_wait()? {
        processes.remove(&id);
        state.update_component(id, |runtime| {
            runtime.lifecycle = LifecycleState::Error;
            runtime.healthy = false;
            runtime.busy = false;
            runtime.pid = None;
            runtime.last_error = Some(format!("进程已退出：{status}"));
        });
        state.log(
            LogSource::from(id),
            LogLevel::Error,
            format!("进程已退出：{status}"),
        );
        Ok(true)
    } else {
        Ok(false)
    }
}

fn prepare_component_data(state: &AppState, id: ComponentId) -> AppResult<()> {
    let data_dir = state.component_data_dir(id);
    fs::create_dir_all(&data_dir)?;
    match id {
        ComponentId::Cliproxyapi => {
            let result = ensure_cliproxy_secret(&data_dir)?;
            state.update_component(id, |runtime| {
                runtime.management_key = result.key.clone();
            });
            if result.generated {
                state.log(
                    LogSource::from(id),
                    LogLevel::Info,
                    "remote-management secret-key 为空，已生成安全随机 key",
                );
            }
        }
        ComponentId::CpaManagerPlus => {
            fs::create_dir_all(data_dir.join("data"))?;
            let config = data_dir.join("config.json");
            if !config.exists() {
                fs::write(
                    config,
                    serde_json::to_vec_pretty(&serde_json::json!({
                        "httpAddr": "127.0.0.1:18317",
                        "dataDir": "./data",
                        "cpaUpstreamUrl": "http://127.0.0.1:8317"
                    }))?,
                )?;
            }
        }
    }
    Ok(())
}

fn ensure_port_available(port: u16) -> AppResult<()> {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpListener::bind(address)
        .map(drop)
        .map_err(|_| message(format!("端口 {port} 已被占用")))
}

async fn service_health(state: &AppState, id: ComponentId) -> bool {
    let definition = definition(id);
    let url = format!(
        "http://127.0.0.1:{}{}",
        definition.port, definition.health_path
    );
    tokio::time::timeout(Duration::from_millis(1200), state.client.get(url).send())
        .await
        .is_ok_and(|result| result.is_ok_and(|response| response.status().is_success()))
}

fn capture_output<R>(state: AppState, id: ComponentId, stream: R, level: LogLevel)
where
    R: std::io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            state.log(LogSource::from(id), level, line);
        }
    });
}
