use serde::Deserialize;

use crate::{
    components::{definition, expected_asset_name},
    error::{message, AppResult},
    models::ComponentId,
};

#[derive(Debug, Clone, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<GithubAsset>,
}

pub async fn latest_release(client: &reqwest::Client, id: ComponentId) -> AppResult<GithubRelease> {
    let repository = definition(id).repository;
    let url = format!("https://api.github.com/repos/{repository}/releases?per_page=10");
    let releases = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<GithubRelease>>()
        .await?;
    releases
        .into_iter()
        .find(|release| {
            !release.draft && !release.prerelease && !is_blocked_release(id, &release.tag_name)
        })
        .ok_or_else(|| message("GitHub 未返回可安装的正式 Release"))
}

pub fn resolve_assets(
    id: ComponentId,
    release: &GithubRelease,
) -> AppResult<(GithubAsset, GithubAsset)> {
    let expected = expected_asset_name(id, &release.tag_name)?;
    let archive = release
        .assets
        .iter()
        .find(|asset| asset.name == expected)
        .cloned()
        .ok_or_else(|| message(format!("未找到当前平台资产：{expected}")))?;
    let checksums = release
        .assets
        .iter()
        .find(|asset| asset.name.eq_ignore_ascii_case("checksums.txt"))
        .cloned()
        .ok_or_else(|| message("Release 未提供 checksums.txt，已阻止自动安装"))?;
    Ok((archive, checksums))
}

pub fn is_update_available(installed: Option<&str>, latest: Option<&str>) -> bool {
    let Some(installed) = installed else {
        return false;
    };
    let Some(latest) = latest else { return false };
    let installed = installed.trim_start_matches('v');
    let latest = latest.trim_start_matches('v');
    match (
        semver::Version::parse(installed),
        semver::Version::parse(latest),
    ) {
        (Ok(installed), Ok(latest)) => latest > installed,
        _ => latest != installed,
    }
}

fn is_blocked_release(id: ComponentId, tag_name: &str) -> bool {
    // CPA-Manager-Plus v1.11.0 Windows amd64 exits during fresh SQLite startup
    // with "SQL logic error: out of memory (1)"; keep installs on v1.10.5 until
    // an upstream fixed release supersedes it.
    // https://github.com/seakee/CPA-Manager-Plus/issues/345
    matches!(
        (id, std::env::consts::OS, tag_name.trim_start_matches('v')),
        (ComponentId::CpaManagerPlus, "windows", "1.11.0")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn blocks_cpa_manager_plus_windows_release_with_sqlite_startup_failure() {
        assert!(is_blocked_release(ComponentId::CpaManagerPlus, "v1.11.0"));
        assert!(!is_blocked_release(ComponentId::CpaManagerPlus, "v1.10.5"));
        assert!(!is_blocked_release(ComponentId::Cliproxyapi, "v1.11.0"));
    }

    #[test]
    #[cfg(not(windows))]
    fn release_blocklist_is_windows_scoped() {
        assert!(!is_blocked_release(ComponentId::CpaManagerPlus, "v1.11.0"));
    }
}
