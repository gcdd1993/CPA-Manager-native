use std::{fs, path::Path};

use crate::error::{message, AppResult};

pub struct SecretKeyResult {
    pub key: Option<String>,
    pub generated: bool,
}

const SECRET_FILE_NAME: &str = "management-secret.txt";

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
}
