use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use chrono::Utc;
use reqwest::{Method, StatusCode, Url};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

use crate::{
    config::{ManagerSettings, WebDavSettings},
    error::{message, AppResult},
    state::AppState,
};

const MAX_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
const ARCHIVE_VERSION: u8 = 1;
const MANAGER_FILE: &str = "manager/settings.json";
const CPA_FILE: &str = "cpa/config.yaml";
const CPAMP_FILE: &str = "cpa-manager-plus/config.json";
const ALLOWED_FILES: [&str; 3] = [MANAGER_FILE, CPA_FILE, CPAMP_FILE];

static SYNC_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Serialize, Deserialize)]
struct SyncManifest {
    version: u8,
    created_at: String,
    files: Vec<String>,
}

fn sync_lock() -> &'static Mutex<()> {
    SYNC_LOCK.get_or_init(|| Mutex::new(()))
}

fn validate(settings: &WebDavSettings) -> AppResult<Url> {
    let url = Url::parse(settings.base_url.trim())
        .map_err(|error| message(format!("WebDAV 地址无效：{error}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(message("WebDAV 地址仅支持 http 或 https"));
    }
    if settings.remote_path.trim_matches('/').is_empty() {
        return Err(message("WebDAV 远端文件路径不能为空"));
    }
    Ok(url)
}

fn request_auth(
    builder: reqwest::RequestBuilder,
    settings: &WebDavSettings,
) -> reqwest::RequestBuilder {
    if settings.username.trim().is_empty() {
        builder
    } else {
        builder.basic_auth(settings.username.trim(), Some(settings.password.as_str()))
    }
}

fn remote_segments(settings: &WebDavSettings) -> Vec<String> {
    settings
        .remote_path
        .trim_matches('/')
        .split('/')
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn append_segments(mut url: Url, segments: &[String]) -> AppResult<Url> {
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| message("WebDAV 地址无法追加远端路径"))?;
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
    }
    Ok(url)
}

pub async fn test(state: &AppState, settings: &WebDavSettings) -> AppResult<()> {
    let url = validate(settings)?;
    let method = Method::from_bytes(b"PROPFIND").expect("valid method");
    let response = request_auth(
        state.client.request(method, url).header("Depth", "0"),
        settings,
    )
    .send()
    .await?;
    if response.status().is_success() || response.status() == StatusCode::MULTI_STATUS {
        Ok(())
    } else {
        Err(status_error("连接", response.status()))
    }
}

async fn ensure_directories(
    state: &AppState,
    settings: &WebDavSettings,
    base: &Url,
    directories: &[String],
) -> AppResult<()> {
    let method = Method::from_bytes(b"MKCOL").expect("valid method");
    for depth in 1..=directories.len() {
        let url = append_segments(base.clone(), &directories[..depth])?;
        let response = request_auth(state.client.request(method.clone(), url), settings)
            .send()
            .await?;
        if !(response.status().is_success() || response.status() == StatusCode::METHOD_NOT_ALLOWED)
        {
            return Err(status_error("创建目录", response.status()));
        }
    }
    Ok(())
}

pub async fn upload(state: &AppState) -> AppResult<()> {
    let _guard = sync_lock().lock().await;
    let settings = state.settings().webdav;
    let base = validate(&settings)?;
    let segments = remote_segments(&settings);
    ensure_directories(state, &settings, &base, &segments[..segments.len() - 1]).await?;
    let archive = build_archive(state)?;
    let url = append_segments(base, &segments)?;
    let response = request_auth(
        state
            .client
            .put(url)
            .header("Content-Type", "application/zip")
            .body(archive),
        &settings,
    )
    .send()
    .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(status_error("上传", response.status()))
    }
}

pub async fn download(state: &AppState) -> AppResult<()> {
    let _guard = sync_lock().lock().await;
    let settings = state.settings().webdav;
    let base = validate(&settings)?;
    let url = append_segments(base, &remote_segments(&settings))?;
    let response = request_auth(state.client.get(url), &settings)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(status_error("下载", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_ARCHIVE_BYTES as u64)
    {
        return Err(message("WebDAV 配置归档超过 32 MB 安全上限"));
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(message("WebDAV 配置归档超过 32 MB 安全上限"));
    }
    restore_archive(state, &bytes)
}

fn build_archive(state: &AppState) -> AppResult<Vec<u8>> {
    let settings_path = state.manager_config_dir().join("settings.json");
    let candidates = [
        (MANAGER_FILE, settings_path),
        (CPA_FILE, state.root().join("config.yaml")),
        (CPAMP_FILE, state.root().join("config.json")),
    ];
    let present: Vec<_> = candidates
        .into_iter()
        .filter(|(_, path)| path.is_file())
        .collect();
    let manifest = SyncManifest {
        version: ARCHIVE_VERSION,
        created_at: Utc::now().to_rfc3339(),
        files: present
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect(),
    };
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer.start_file("manifest.json", options)?;
    writer.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    for (name, path) in present {
        writer.start_file(name, options)?;
        if name == MANAGER_FILE {
            let mut manager = state.settings();
            // A WebDAV password authenticates this upload and must not be copied into it.
            manager.webdav.password.clear();
            manager.webdav.last_error = None;
            writer.write_all(&serde_json::to_vec_pretty(&manager)?)?;
        } else {
            writer.write_all(&fs::read(path)?)?;
        }
    }
    Ok(writer.finish()?.into_inner())
}

fn restore_archive(state: &AppState, bytes: &[u8]) -> AppResult<()> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let manifest: SyncManifest = {
        let mut entry = archive
            .by_name("manifest.json")
            .map_err(|_| message("归档缺少 manifest.json"))?;
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        serde_json::from_slice(&data)?
    };
    if manifest.version != ARCHIVE_VERSION {
        return Err(message(format!(
            "不支持的 WebDAV 配置归档版本：{}",
            manifest.version
        )));
    }
    validate_manifest_files(&manifest.files)?;
    validate_archive_entries(&mut archive, &manifest.files)?;
    // Read and validate every entry before touching local files. A malformed archive
    // therefore cannot leave a half-restored configuration set behind.
    let mut restored = Vec::new();
    for name in &manifest.files {
        let mut entry = archive
            .by_name(name)
            .map_err(|_| message(format!("归档缺少声明的文件：{name}")))?;
        if entry.size() > MAX_ARCHIVE_BYTES as u64 {
            return Err(message("归档内配置文件超过安全上限"));
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        if name.ends_with(".json") {
            let _: serde_json::Value = serde_json::from_slice(&data)?;
        }
        restored.push((name.clone(), data));
    }
    let backup_root = state
        .manager_config_dir()
        .join("webdav-backups")
        .join(Utc::now().format("%Y%m%d-%H%M%S").to_string());
    for (name, data) in &restored {
        let target = target_path(state, name)?;
        if target.is_file() {
            let backup = backup_root.join(name);
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&target, backup)?;
        }
        atomic_write(&target, data)?;
    }
    if manifest.files.iter().any(|name| name == MANAGER_FILE) {
        let mut imported: ManagerSettings =
            serde_json::from_slice(&fs::read(state.manager_config_dir().join("settings.json"))?)?;
        // Absolute data paths and connection credentials are machine-local.
        let local = state.settings();
        imported.data_directory = local.data_directory;
        imported.webdav = local.webdav;
        state.replace_settings(imported)?;
    }
    Ok(())
}

fn validate_archive_entries(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    files: &[String],
) -> AppResult<()> {
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let name = entry.name();
        if name != "manifest.json" && !files.iter().any(|allowed| allowed == name) {
            return Err(message("归档包含 manifest 未声明的文件"));
        }
    }
    Ok(())
}

fn validate_manifest_files(files: &[String]) -> AppResult<()> {
    if files.len() > ALLOWED_FILES.len()
        || files
            .iter()
            .any(|name| !ALLOWED_FILES.contains(&name.as_str()))
    {
        return Err(message("归档包含不允许同步的文件"));
    }
    let mut unique = files.to_vec();
    unique.sort();
    unique.dedup();
    if unique.len() != files.len() {
        return Err(message("归档包含重复配置文件"));
    }
    Ok(())
}

fn target_path(state: &AppState, name: &str) -> AppResult<PathBuf> {
    match name {
        MANAGER_FILE => Ok(state.manager_config_dir().join("settings.json")),
        CPA_FILE => Ok(state.root().join("config.yaml")),
        CPAMP_FILE => Ok(state.root().join("config.json")),
        _ => Err(message("归档路径不在配置白名单中")),
    }
}

fn atomic_write(path: &Path, data: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!(
        "{}.webdav.tmp",
        path.extension()
            .and_then(|v| v.to_str())
            .unwrap_or("config")
    ));
    fs::write(&temporary, data)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn status_error(operation: &str, status: StatusCode) -> crate::error::AppError {
    let hint = if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        "，请检查用户名、应用密码和目录权限"
    } else {
        ""
    };
    message(format!("WebDAV {operation}失败：{status}{hint}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_path_is_split_without_empty_segments() {
        let settings = WebDavSettings {
            remote_path: "/CPA Manager/config.zip/".into(),
            ..Default::default()
        };
        assert_eq!(
            remote_segments(&settings),
            vec!["CPA Manager", "config.zip"]
        );
    }
    #[test]
    fn whitelist_contains_only_three_configuration_files() {
        assert_eq!(ALLOWED_FILES, [MANAGER_FILE, CPA_FILE, CPAMP_FILE]);
        assert!(ALLOWED_FILES
            .iter()
            .all(|name| !name.contains("components") && !name.ends_with(".exe")));
    }
    #[test]
    fn manifest_rejects_program_and_traversal_paths() {
        assert!(validate_manifest_files(&["components/cliproxyapi/app.exe".into()]).is_err());
        assert!(validate_manifest_files(&["../settings.json".into()]).is_err());
        assert!(validate_manifest_files(&[MANAGER_FILE.into(), MANAGER_FILE.into()]).is_err());
    }
}
