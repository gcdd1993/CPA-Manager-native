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
    reclaim_port(state, id, definition.port).await?;

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

fn port_is_available(port: u16) -> bool {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpListener::bind(address).map(drop).is_ok()
}

async fn reclaim_port(state: &AppState, id: ComponentId, port: u16) -> AppResult<()> {
    if port_is_available(port) {
        return Ok(());
    }

    let current_pid = std::process::id();
    let pids: Vec<u32> = listener_pids(port)?
        .into_iter()
        .filter(|pid| *pid != current_pid)
        .collect();
    if pids.is_empty() {
        return Err(message(format!(
            "端口 {port} 已被占用，但未能识别可停止的监听进程"
        )));
    }

    let pid_list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join("、");
    state.log(
        LogSource::from(id),
        LogLevel::Warn,
        format!("端口 {port} 被 PID {pid_list} 占用，正在自动停止占用进程"),
    );

    let mut stop_errors = Vec::new();
    for pid in &pids {
        if let Err(error) = stop_port_process(*pid, false) {
            stop_errors.push(format!("PID {pid}: {error}"));
        }
    }

    if wait_for_port(port, Duration::from_millis(1500)).await {
        state.log(
            LogSource::from(id),
            LogLevel::Info,
            format!("端口 {port} 已释放，继续启动组件"),
        );
        return Ok(());
    }

    #[cfg(not(windows))]
    for pid in &pids {
        if let Err(error) = stop_port_process(*pid, true) {
            stop_errors.push(format!("强制停止 PID {pid}: {error}"));
        }
    }

    if wait_for_port(port, Duration::from_millis(3500)).await {
        state.log(
            LogSource::from(id),
            LogLevel::Info,
            format!("端口 {port} 已释放，继续启动组件"),
        );
        return Ok(());
    }

    let details = if stop_errors.is_empty() {
        String::new()
    } else {
        format!("：{}", stop_errors.join("；"))
    };
    Err(message(format!(
        "端口 {port} 仍被 PID {pid_list} 占用，自动停止失败{details}"
    )))
}

async fn wait_for_port(port: u16, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if port_is_available(port) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(windows)]
fn listener_pids(port: u16) -> AppResult<Vec<u32>> {
    let output = Command::new("netstat")
        .args(["-ano", "-p", "tcp"])
        .creation_flags(0x08000000)
        .output()
        .map_err(|error| message(format!("无法查询端口 {port} 的占用进程：{error}")))?;
    if !output.status.success() {
        return Err(message(format!("无法查询端口 {port} 的占用进程")));
    }
    Ok(parse_windows_listeners(
        &String::from_utf8_lossy(&output.stdout),
        port,
    ))
}

#[cfg(windows)]
fn parse_windows_listeners(output: &str, port: u16) -> Vec<u32> {
    let port_suffix = format!(":{port}");
    let mut pids = output
        .lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 5
                || !fields[0].eq_ignore_ascii_case("TCP")
                || !fields[1].ends_with(&port_suffix)
                || !fields[fields.len() - 2].eq_ignore_ascii_case("LISTENING")
            {
                return None;
            }
            fields.last()?.parse::<u32>().ok()
        })
        .collect::<Vec<_>>();
    pids.sort_unstable();
    pids.dedup();
    pids
}

#[cfg(target_os = "linux")]
fn listener_pids(port: u16) -> AppResult<Vec<u32>> {
    let output = Command::new("fuser")
        .args(["-n", "tcp", &port.to_string()])
        .output()
        .map_err(|error| message(format!("无法查询端口 {port} 的占用进程：{error}")))?;
    Ok(parse_pid_list(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(target_os = "macos")]
fn listener_pids(port: u16) -> AppResult<Vec<u32>> {
    let output = Command::new("lsof")
        .args(["-nP", "-t", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
        .output()
        .map_err(|error| message(format!("无法查询端口 {port} 的占用进程：{error}")))?;
    Ok(parse_pid_list(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(not(windows))]
fn parse_pid_list(output: &str) -> Vec<u32> {
    let mut pids = output
        .split_whitespace()
        .filter_map(|value| value.parse::<u32>().ok())
        .collect::<Vec<_>>();
    pids.sort_unstable();
    pids.dedup();
    pids
}

#[cfg(windows)]
fn stop_port_process(pid: u32, _force: bool) -> AppResult<()> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(0x08000000)
        .status()
        .map_err(|error| message(format!("无法结束进程：{error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(message(format!("taskkill 退出状态 {status}")))
    }
}

#[cfg(not(windows))]
fn stop_port_process(pid: u32, force: bool) -> AppResult<()> {
    let signal = if force { "-KILL" } else { "-TERM" };
    let status = Command::new("kill")
        .args([signal, &pid.to_string()])
        .status()
        .map_err(|error| message(format!("无法结束进程：{error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(message(format!("kill 退出状态 {status}")))
    }
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

#[cfg(all(test, windows))]
mod tests {
    use super::parse_windows_listeners;

    #[test]
    fn parses_only_matching_tcp_listeners_and_deduplicates_pids() {
        let output = r#"
  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:8317           0.0.0.0:0              LISTENING       4321
  TCP    [::]:8317              [::]:0                 LISTENING       4321
  TCP    127.0.0.1:18317        0.0.0.0:0              LISTENING       9876
  TCP    127.0.0.1:8317         127.0.0.1:50000        ESTABLISHED     5555
  UDP    0.0.0.0:8317           *:*                                    2468
"#;

        assert_eq!(parse_windows_listeners(output, 8317), vec![4321]);
        assert_eq!(parse_windows_listeners(output, 18317), vec![9876]);
    }
}
