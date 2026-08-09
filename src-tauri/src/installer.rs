use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use chrono::Utc;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use crate::{
    components::locate_executable,
    error::{message, AppResult},
    github::{latest_release, resolve_assets},
    models::{ComponentId, InstallManifest, LifecycleState, LogLevel, LogSource},
    process,
    state::{component_root, write_manifest, AppState},
};

pub async fn install(app: &AppHandle, state: &AppState, id: ComponentId) -> AppResult<()> {
    let was_running =
        state.with_component(id, |runtime| runtime.lifecycle == LifecycleState::Running);
    state.update_component(id, |runtime| {
        runtime.lifecycle = if runtime.installed.is_some() {
            LifecycleState::Updating
        } else {
            LifecycleState::Installing
        };
        runtime.busy = true;
        runtime.progress_percent = Some(0);
        runtime.progress_label = Some("查询 GitHub Release".into());
        runtime.last_error = None;
    });
    state.emit_snapshot(app);

    let release = latest_release(&state.client, id).await?;
    state.set_latest_version(id, release.tag_name.clone());
    let (archive, checksums) = resolve_assets(id, &release)?;
    state.log(
        LogSource::from(id),
        LogLevel::Info,
        format!(
            "匹配 Release 资产：{} ({} bytes)",
            archive.name, archive.size
        ),
    );

    let checksum_text = state
        .client
        .get(&checksums.browser_download_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let expected_hash = parse_checksum(&checksum_text, &archive.name)
        .ok_or_else(|| message(format!("checksums.txt 中未找到 {}", archive.name)))?;

    let data_root = state.root();
    let download_path = data_root
        .join("downloads")
        .join(format!("{}.part", archive.name));
    download(
        app,
        state,
        id,
        &archive.browser_download_url,
        &download_path,
        archive.size,
    )
    .await?;

    set_progress(app, state, id, 72, "校验 SHA-256");
    let actual_hash = tokio::task::spawn_blocking({
        let path = download_path.clone();
        move || sha256_file(&path)
    })
    .await
    .map_err(|error| message(format!("校验任务失败：{error}")))??;
    if !actual_hash.eq_ignore_ascii_case(&expected_hash) {
        let _ = fs::remove_file(&download_path);
        return Err(message(format!(
            "SHA-256 校验失败，期望 {expected_hash}，实际 {actual_hash}"
        )));
    }

    let version = release.tag_name.trim_start_matches('v').to_string();
    let data_root = state.root();
    let root = component_root(&data_root, id);
    fs::create_dir_all(root.join("versions"))?;
    let staging = root.join(format!("staging-{version}"));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    set_progress(app, state, id, 82, "安全解压安装包");
    tokio::task::spawn_blocking({
        let archive_path = download_path.clone();
        let staging_path = staging.clone();
        move || extract_archive(&archive_path, &staging_path)
    })
    .await
    .map_err(|error| message(format!("解压任务失败：{error}")))??;

    let executable = locate_executable(&staging, id)?;
    set_executable_permissions(&executable)?;
    let executable_relative = executable
        .strip_prefix(&staging)
        .map_err(|_| message("无法计算可执行文件相对路径"))?
        .to_path_buf();

    if was_running {
        set_progress(app, state, id, 88, "停止旧版本");
        process::stop(state, id).await?;
    }

    let version_dir = root.join("versions").join(&version);
    if version_dir.exists() {
        fs::remove_dir_all(&version_dir)?;
    }
    fs::rename(&staging, &version_dir)?;
    let manifest = InstallManifest {
        version: release.tag_name.clone(),
        asset_name: archive.name,
        download_url: archive.browser_download_url,
        sha256: actual_hash,
        installed_at: Utc::now().to_rfc3339(),
        executable_path: version_dir.join(executable_relative),
    };
    write_manifest(&state.current_manifest_path(id), &manifest)?;
    state.update_component(id, |runtime| {
        runtime.installed = Some(manifest);
        runtime.lifecycle = LifecycleState::Stopped;
        runtime.healthy = false;
        runtime.pid = None;
        runtime.busy = false;
        runtime.progress_percent = Some(100);
        runtime.progress_label = Some("安装完成".into());
        runtime.last_error = None;
    });
    let _ = fs::remove_file(download_path);
    cleanup_old_versions(&root, &version, 2)?;
    state.log(
        LogSource::from(id),
        LogLevel::Info,
        format!("版本 {} 安装完成", release.tag_name),
    );

    if was_running {
        process::start(app, state, id).await?;
    }
    state.emit_snapshot(app);
    Ok(())
}

async fn download(
    app: &AppHandle,
    state: &AppState,
    id: ComponentId,
    url: &str,
    path: &Path,
    expected_size: u64,
) -> AppResult<()> {
    set_progress(app, state, id, 5, "下载 Release 资产");
    let response = state
        .download_client
        .get(url)
        .send()
        .await?
        .error_for_status()?;
    let total = response.content_length().unwrap_or(expected_size).max(1);
    let mut file = tokio::fs::File::create(path).await?;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0u64;
    let mut last_percent = 0u8;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        let percent = 5 + ((downloaded.saturating_mul(65) / total).min(65) as u8);
        if percent >= last_percent.saturating_add(2) {
            last_percent = percent;
            set_progress(app, state, id, percent, "下载 Release 资产");
        }
    }
    file.flush().await?;
    Ok(())
}

fn set_progress(app: &AppHandle, state: &AppState, id: ComponentId, percent: u8, label: &str) {
    state.update_component(id, |runtime| {
        runtime.busy = true;
        runtime.progress_percent = Some(percent);
        runtime.progress_label = Some(label.to_string());
    });
    state.emit_snapshot(app);
}

pub fn parse_checksum(contents: &str, asset_name: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        let file_name = file.rsplit(['/', '\\']).next()?;
        if file_name == asset_name
            && hash.len() == 64
            && hash.chars().all(|value| value.is_ascii_hexdigit())
        {
            Some(hash.to_string())
        } else {
            None
        }
    })
}

fn sha256_file(path: &Path) -> AppResult<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn extract_archive(archive_path: &Path, destination: &Path) -> AppResult<()> {
    let name = archive_path.to_string_lossy();
    if name.ends_with(".zip.part") || name.ends_with(".zip") {
        let file = fs::File::open(archive_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let relative = entry
                .enclosed_name()
                .ok_or_else(|| message(format!("压缩包包含不安全路径：{}", entry.name())))?
                .to_path_buf();
            let output = destination.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&output)?;
                continue;
            }
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut target = fs::File::create(output)?;
            std::io::copy(&mut entry, &mut target)?;
        }
        return Ok(());
    }
    if name.ends_with(".tar.gz.part") || name.ends_with(".tar.gz") {
        let file = fs::File::open(archive_path)?;
        let decoder = GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let kind = entry.header().entry_type();
            if kind.is_symlink() || kind.is_hard_link() {
                return Err(message("压缩包包含符号链接或硬链接，已拒绝解压"));
            }
            if !entry.unpack_in(destination)? {
                return Err(message("压缩包尝试写出目标目录，已拒绝解压"));
            }
        }
        return Ok(());
    }
    Err(message("不支持的压缩格式"))
}

fn set_executable_permissions(_path: &Path) -> AppResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(_path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(_path, permissions)?;
    }
    Ok(())
}

fn cleanup_old_versions(root: &Path, current: &str, keep: usize) -> AppResult<()> {
    let versions_root = root.join("versions");
    let mut versions: Vec<(PathBuf, std::time::SystemTime)> = fs::read_dir(&versions_root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| {
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (entry.path(), modified)
        })
        .collect();
    versions.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    let mut retained = 0usize;
    for (path, _) in versions {
        let is_current = path.file_name().and_then(|name| name.to_str()) == Some(current);
        if is_current || retained < keep.saturating_sub(1) {
            if !is_current {
                retained += 1;
            }
            continue;
        }
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_and_starred_checksums() {
        let name = "asset.zip";
        let hash = "a".repeat(64);
        assert_eq!(
            parse_checksum(&format!("{hash}  {name}"), name),
            Some(hash.clone())
        );
        assert_eq!(parse_checksum(&format!("{hash} *{name}"), name), Some(hash));
    }

    #[test]
    fn parses_release_checksums_with_relative_paths() {
        let name = "cpa-manager-plus_v1.10.5_windows_amd64.zip";
        let hash = "b".repeat(64);
        assert_eq!(
            parse_checksum(&format!("{hash}  ./{name}"), name),
            Some(hash.clone())
        );
        assert_eq!(
            parse_checksum(&format!("{hash}  .\\{name}"), name),
            Some(hash)
        );
    }
}
