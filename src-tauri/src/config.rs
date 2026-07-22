use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::error::{message, AppResult};

pub struct SecretKeyResult {
    pub key: Option<String>,
    pub generated: bool,
}

const SECRET_FILE_NAME: &str = "management-secret.txt";
const MANAGER_CONFIG_DIR_NAME: &str = ".cpamanager-native";
const MANAGER_SETTINGS_FILE_NAME: &str = "settings.json";
const CLIPROXY_LOCAL_HOST: &str = "127.0.0.1";
const CLIPROXY_LAN_HOST: &str = "0.0.0.0";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManagerSettings {
    pub data_directory: PathBuf,
    #[serde(default)]
    pub launch_at_startup: bool,
    #[serde(default)]
    pub webdav: WebDavSettings,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebDavSettings {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_webdav_remote_path")]
    pub remote_path: String,
    #[serde(default)]
    pub last_sync_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

fn default_webdav_remote_path() -> String {
    "CPA-Manager-Native/config-backup.zip".to_string()
}

impl Default for WebDavSettings {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            username: String::new(),
            password: String::new(),
            remote_path: default_webdav_remote_path(),
            last_sync_at: None,
            last_error: None,
        }
    }
}

impl ManagerSettings {
    pub fn new(data_directory: PathBuf) -> Self {
        Self {
            data_directory,
            launch_at_startup: false,
            webdav: WebDavSettings::default(),
        }
    }
}

pub fn manager_config_dir() -> AppResult<PathBuf> {
    let home = user_home_dir().ok_or_else(|| message("无法定位用户 HOME 目录"))?;
    Ok(home.join(MANAGER_CONFIG_DIR_NAME))
}

pub fn load_manager_settings(
    config_dir: &Path,
    default_data_directory: &Path,
) -> AppResult<ManagerSettings> {
    fs::create_dir_all(config_dir)?;
    let path = manager_settings_path(config_dir);
    if !path.exists() {
        let settings = ManagerSettings::new(default_data_directory.to_path_buf());
        write_manager_settings(config_dir, &settings)?;
        return Ok(settings);
    }

    let data = fs::read(&path)?;
    let mut settings: ManagerSettings = serde_json::from_slice(&data)?;
    if settings.data_directory.as_os_str().is_empty() || !settings.data_directory.is_absolute() {
        settings.data_directory = default_data_directory.to_path_buf();
        write_manager_settings(config_dir, &settings)?;
    }
    Ok(settings)
}

pub fn write_manager_settings(config_dir: &Path, settings: &ManagerSettings) -> AppResult<()> {
    fs::create_dir_all(config_dir)?;
    let path = manager_settings_path(config_dir);
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(settings)?)?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn manager_settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join(MANAGER_SETTINGS_FILE_NAME)
}

fn user_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            #[cfg(windows)]
            {
                env::var_os("USERPROFILE")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
            }
            #[cfg(not(windows))]
            {
                None
            }
        })
}

pub fn ensure_cliproxy_secret(data_dir: &Path) -> AppResult<SecretKeyResult> {
    fs::create_dir_all(data_dir)?;
    fs::create_dir_all(data_dir.join("auth"))?;
    let path = data_dir.join("config.yaml");
    let secret_path = data_dir.join(SECRET_FILE_NAME);

    if !path.exists() {
        let saved_key = read_secret_file(&secret_path);
        let generated = saved_key.is_none();
        let key = match saved_key {
            Some(key) => key,
            None => generate_secret_key()?,
        };
        fs::write(&path, default_cliproxy_config(&key))?;
        if generated {
            write_secret_file(&secret_path, &key)?;
        }
        return Ok(SecretKeyResult {
            key: Some(key),
            generated,
        });
    }

    let current = fs::read_to_string(&path)?;
    let normalized = disable_panel_auto_update(&current);
    if normalized != current {
        replace_config_contents(&path, &normalized)?;
    }

    if let Some(key) = read_secret_file(&secret_path) {
        let contents = fs::read_to_string(&path)?;
        if read_secret_key(&contents).is_none() {
            if contents.trim().is_empty() {
                fs::write(&path, default_cliproxy_config(&key))?;
            } else {
                replace_config_secret(&path, &contents, &key)?;
            }
        }
        return Ok(SecretKeyResult {
            key: Some(key),
            generated: false,
        });
    }

    let contents = fs::read_to_string(&path)?;
    if let Some(key) = read_secret_key(&contents) {
        if is_password_hash(&key) {
            return Ok(SecretKeyResult {
                key: None,
                generated: false,
            });
        }
        write_secret_file(&secret_path, &key)?;
        return Ok(SecretKeyResult {
            key: Some(key),
            generated: false,
        });
    }

    let key = generate_secret_key()?;
    replace_config_secret(&path, &contents, &key)?;
    write_secret_file(&secret_path, &key)?;
    Ok(SecretKeyResult {
        key: Some(key),
        generated: true,
    })
}

pub fn read_cliproxy_secret(data_dir: &Path) -> Option<String> {
    read_secret_file(&data_dir.join(SECRET_FILE_NAME)).or_else(|| {
        let key = fs::read_to_string(data_dir.join("config.yaml"))
            .ok()
            .and_then(|contents| read_secret_key(&contents))?;
        (!is_password_hash(&key)).then_some(key)
    })
}

pub fn cliproxy_lan_access_enabled(data_dir: &Path) -> bool {
    fs::read_to_string(data_dir.join("config.yaml"))
        .ok()
        .is_some_and(|contents| {
            read_cliproxy_host(&contents).as_deref() == Some(CLIPROXY_LAN_HOST)
                && read_remote_management_access(&contents) == Some(true)
        })
}

pub fn set_cliproxy_lan_access(data_dir: &Path, enabled: bool) -> AppResult<()> {
    ensure_cliproxy_secret(data_dir)?;
    let path = data_dir.join("config.yaml");
    let contents = fs::read_to_string(&path)?;
    let host = if enabled {
        CLIPROXY_LAN_HOST
    } else {
        CLIPROXY_LOCAL_HOST
    };
    let updated = write_cliproxy_host(&contents, host);
    let updated = write_remote_management_access(&updated, enabled);
    if updated != contents {
        replace_config_contents(&path, &updated)?;
    }
    Ok(())
}

fn read_cliproxy_host(contents: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        if line != line.trim_start() || !line.starts_with("host:") {
            return None;
        }
        let value = line
            .trim_start_matches("host:")
            .split('#')
            .next()?
            .trim()
            .trim_matches(['\'', '"']);
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn write_cliproxy_host(contents: &str, host: &str) -> String {
    let mut lines: Vec<String> = contents.lines().map(ToOwned::to_owned).collect();
    if let Some(index) = lines
        .iter()
        .position(|line| line == line.trim_start() && line.starts_with("host:"))
    {
        lines[index] = format!("host: \"{host}\"");
    } else {
        lines.insert(0, format!("host: \"{host}\""));
    }
    format!("{}\n", lines.join("\n"))
}

fn read_remote_management_access(contents: &str) -> Option<bool> {
    let mut in_remote_management = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        let indentation = line.len().saturating_sub(line.trim_start().len());
        if indentation == 0 {
            in_remote_management = trimmed == "remote-management:";
            continue;
        }
        if in_remote_management && trimmed.starts_with("allow-remote:") {
            let value = trimmed
                .trim_start_matches("allow-remote:")
                .split('#')
                .next()?
                .trim();
            return match value {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
        }
    }
    None
}

fn write_remote_management_access(contents: &str, enabled: bool) -> String {
    let mut lines: Vec<String> = contents.lines().map(ToOwned::to_owned).collect();
    let value = if enabled { "true" } else { "false" };
    let remote_index = lines
        .iter()
        .position(|line| line.trim() == "remote-management:" && line == line.trim_start());

    if let Some(remote_index) = remote_index {
        let section_end = lines
            .iter()
            .enumerate()
            .skip(remote_index + 1)
            .find(|(_, line)| !line.trim().is_empty() && line == &line.trim_start())
            .map(|(index, _)| index)
            .unwrap_or(lines.len());
        if let Some(option_index) = (remote_index + 1..section_end)
            .find(|index| lines[*index].trim().starts_with("allow-remote:"))
        {
            let indentation = &lines[option_index]
                [..lines[option_index].len() - lines[option_index].trim_start().len()];
            lines[option_index] = format!("{indentation}allow-remote: {value}");
        } else {
            lines.insert(remote_index + 1, format!("  allow-remote: {value}"));
        }
    } else {
        if !lines.last().is_none_or(|line| line.is_empty()) {
            lines.push(String::new());
        }
        lines.extend([
            "remote-management:".to_string(),
            format!("  allow-remote: {value}"),
        ]);
    }
    format!("{}\n", lines.join("\n"))
}

fn replace_config_secret(path: &Path, contents: &str, key: &str) -> AppResult<()> {
    let updated = write_secret_key(contents, key);
    replace_config_contents(path, &updated)
}

fn replace_config_contents(path: &Path, contents: &str) -> AppResult<()> {
    let temporary = path.with_extension("yaml.tmp");
    fs::write(&temporary, contents)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn read_secret_file(path: &Path) -> Option<String> {
    let key = fs::read_to_string(path).ok()?;
    let key = key.trim();
    (!key.is_empty()).then(|| key.to_string())
}

fn write_secret_file(path: &Path, key: &str) -> AppResult<()> {
    fs::write(path, format!("{key}\n"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn is_password_hash(value: &str) -> bool {
    value.starts_with("$2a$") || value.starts_with("$2b$") || value.starts_with("$2y$")
}

fn read_secret_key(contents: &str) -> Option<String> {
    let mut in_remote_management = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        let indentation = line.len().saturating_sub(line.trim_start().len());
        if indentation == 0 {
            in_remote_management = trimmed == "remote-management:";
            continue;
        }
        if in_remote_management && trimmed.starts_with("secret-key:") {
            let value = trimmed
                .trim_start_matches("secret-key:")
                .split('#')
                .next()?
                .trim()
                .trim_matches(['\'', '"']);
            return (!value.is_empty()).then(|| value.to_string());
        }
    }
    None
}

fn write_secret_key(contents: &str, key: &str) -> String {
    let mut lines: Vec<String> = contents.lines().map(ToOwned::to_owned).collect();
    let remote_index = lines
        .iter()
        .position(|line| line.trim() == "remote-management:" && line == line.trim_start());

    if let Some(remote_index) = remote_index {
        let section_end = lines
            .iter()
            .enumerate()
            .skip(remote_index + 1)
            .find(|(_, line)| !line.trim().is_empty() && line == &line.trim_start())
            .map(|(index, _)| index)
            .unwrap_or(lines.len());
        if let Some(secret_index) = (remote_index + 1..section_end)
            .find(|index| lines[*index].trim().starts_with("secret-key:"))
        {
            let indentation = &lines[secret_index]
                [..lines[secret_index].len() - lines[secret_index].trim_start().len()];
            lines[secret_index] = format!("{indentation}secret-key: \"{key}\"");
        } else {
            lines.insert(remote_index + 1, format!("  secret-key: \"{key}\""));
        }
    } else {
        if !lines.last().is_none_or(|line| line.is_empty()) {
            lines.push(String::new());
        }
        lines.extend([
            "remote-management:".to_string(),
            "  allow-remote: false".to_string(),
            format!("  secret-key: \"{key}\""),
        ]);
    }
    format!("{}\n", lines.join("\n"))
}

fn disable_panel_auto_update(contents: &str) -> String {
    let mut lines: Vec<String> = contents.lines().map(ToOwned::to_owned).collect();
    let remote_index = lines
        .iter()
        .position(|line| line.trim() == "remote-management:" && line == line.trim_start());

    if let Some(remote_index) = remote_index {
        let section_end = lines
            .iter()
            .enumerate()
            .skip(remote_index + 1)
            .find(|(_, line)| !line.trim().is_empty() && line == &line.trim_start())
            .map(|(index, _)| index)
            .unwrap_or(lines.len());
        if let Some(option_index) = (remote_index + 1..section_end).find(|index| {
            lines[*index]
                .trim()
                .starts_with("disable-auto-update-panel:")
        }) {
            let indentation = &lines[option_index]
                [..lines[option_index].len() - lines[option_index].trim_start().len()];
            lines[option_index] = format!("{indentation}disable-auto-update-panel: true");
        } else {
            lines.insert(
                remote_index + 1,
                "  disable-auto-update-panel: true".to_string(),
            );
        }
    } else {
        if !lines.last().is_none_or(|line| line.is_empty()) {
            lines.push(String::new());
        }
        lines.extend([
            "remote-management:".to_string(),
            "  allow-remote: false".to_string(),
            "  disable-auto-update-panel: true".to_string(),
        ]);
    }
    format!("{}\n", lines.join("\n"))
}

fn generate_secret_key() -> AppResult<String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes)
        .map_err(|error| message(format!("无法生成安全随机密钥：{error}")))?;
    let encoded = bytes
        .iter()
        .map(|value| format!("{value:02x}"))
        .collect::<String>();
    Ok(format!("cpa_{encoded}"))
}

fn default_cliproxy_config(key: &str) -> String {
    format!(
        "host: \"127.0.0.1\"\nport: 8317\nremote-management:\n  allow-remote: false\n  disable-auto-update-panel: true\n  secret-key: \"{key}\"\nauth-dir: \"auth\"\napi-keys:\n  - \"cpa-local-key\"\ndebug: false\nlogging-to-file: false\nusage-statistics-enabled: true\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_preserves_existing_secret() {
        let config = "remote-management:\n  allow-remote: false\n  secret-key: \"existing-key\"\nport: 8317\n";
        assert_eq!(read_secret_key(config).as_deref(), Some("existing-key"));
    }

    #[test]
    fn fills_empty_secret_without_rewriting_other_settings() {
        let config = "host: \"127.0.0.1\"\nremote-management:\n  allow-remote: false\n  secret-key: \"\"\nport: 8317\n";
        let updated = write_secret_key(config, "generated-key");
        assert!(updated.contains("secret-key: \"generated-key\""));
        assert!(updated.contains("host: \"127.0.0.1\""));
        assert!(updated.contains("port: 8317"));
    }

    #[test]
    fn generated_secret_is_persisted_and_reused() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("config.yaml"),
            "remote-management:\n  allow-remote: false\n  secret-key: \"\"\n",
        )
        .unwrap();

        let first = ensure_cliproxy_secret(directory.path()).unwrap();
        let second = ensure_cliproxy_secret(directory.path()).unwrap();

        assert!(first.generated);
        assert!(!second.generated);
        assert_eq!(first.key, second.key);
        assert!(first.key.unwrap().starts_with("cpa_"));
    }

    #[test]
    fn does_not_expose_or_rotate_unowned_password_hash() {
        let directory = tempfile::tempdir().unwrap();
        let hash = "$2a$10$aUQO6963VX8NqfHZ8LV3seyzIRT5iranSUI7nqgkQBtkxfhEMkyaa";
        fs::write(
            directory.path().join("config.yaml"),
            format!("remote-management:\n  secret-key: \"{hash}\"\n"),
        )
        .unwrap();

        let result = ensure_cliproxy_secret(directory.path()).unwrap();
        assert!(!result.generated);
        assert!(result.key.is_none());
        assert!(fs::read_to_string(directory.path().join("config.yaml"))
            .unwrap()
            .contains(hash));
    }

    #[test]
    fn recreates_deleted_config_with_saved_secret() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(SECRET_FILE_NAME), "cpa_saved_key\n").unwrap();

        let result = ensure_cliproxy_secret(directory.path()).unwrap();
        let config = fs::read_to_string(directory.path().join("config.yaml")).unwrap();

        assert!(!result.generated);
        assert_eq!(result.key.as_deref(), Some("cpa_saved_key"));
        assert!(config.contains("host: \"127.0.0.1\""));
        assert!(config.contains("secret-key: \"cpa_saved_key\""));
    }

    #[test]
    fn disables_cliproxy_management_panel_auto_updates() {
        let config = "remote-management:\n  allow-remote: false\n  disable-auto-update-panel: false\n  secret-key: \"key\"\n";
        let updated = disable_panel_auto_update(config);
        assert!(updated.contains("disable-auto-update-panel: true"));
        assert!(!updated.contains("disable-auto-update-panel: false"));
    }

    #[test]
    fn reads_lan_access_from_top_level_host() {
        let config = "host: \"0.0.0.0\"\nproxy:\n  host: \"127.0.0.1\"\n";
        assert_eq!(read_cliproxy_host(config).as_deref(), Some("0.0.0.0"));
    }

    #[test]
    fn toggles_cliproxy_host_without_touching_nested_hosts() {
        let config = "host: \"127.0.0.1\"\nproxy:\n  host: \"upstream.local\"\n";
        let enabled = write_cliproxy_host(config, CLIPROXY_LAN_HOST);
        assert!(enabled.starts_with("host: \"0.0.0.0\"\n"));
        assert!(enabled.contains("  host: \"upstream.local\""));

        let disabled = write_cliproxy_host(&enabled, CLIPROXY_LOCAL_HOST);
        assert!(disabled.starts_with("host: \"127.0.0.1\"\n"));
    }

    #[test]
    fn inserts_missing_cliproxy_host_at_the_top() {
        let updated = write_cliproxy_host("port: 8317\n", CLIPROXY_LAN_HOST);
        assert_eq!(updated, "host: \"0.0.0.0\"\nport: 8317\n");
    }

    #[test]
    fn persists_lan_access_in_cliproxy_config() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("config.yaml"),
            "host: \"127.0.0.1\"\nport: 8317\nremote-management:\n  secret-key: \"key\"\n",
        )
        .unwrap();

        set_cliproxy_lan_access(directory.path(), true).unwrap();
        assert!(cliproxy_lan_access_enabled(directory.path()));
        assert!(fs::read_to_string(directory.path().join("config.yaml"))
            .unwrap()
            .contains("  allow-remote: true"));

        set_cliproxy_lan_access(directory.path(), false).unwrap();
        assert!(!cliproxy_lan_access_enabled(directory.path()));
        let disabled = fs::read_to_string(directory.path().join("config.yaml")).unwrap();
        assert!(disabled.starts_with("host: \"127.0.0.1\"\n"));
        assert!(disabled.contains("  allow-remote: false"));
    }

    #[test]
    fn inserts_missing_remote_management_access_setting() {
        let config = "host: \"0.0.0.0\"\nremote-management:\n  secret-key: \"key\"\n";
        let updated = write_remote_management_access(config, true);
        assert!(updated.contains("remote-management:\n  allow-remote: true\n"));
        assert_eq!(read_remote_management_access(&updated), Some(true));
    }
}
