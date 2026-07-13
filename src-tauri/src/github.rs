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
    let url = format!("https://api.github.com/repos/{repository}/releases/latest");
    let release = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json::<GithubRelease>()
        .await?;
    if release.draft || release.prerelease {
        return Err(message("GitHub 返回的 latest Release 不是正式版本"));
    }
    Ok(release)
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
