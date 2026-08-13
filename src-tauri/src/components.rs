use std::path::{Path, PathBuf};

use crate::{
    error::{message, AppResult},
    models::ComponentId,
};

#[derive(Debug, Clone, Copy)]
pub struct ComponentDefinition {
    pub id: ComponentId,
    pub name: &'static str,
    pub short_name: &'static str,
    pub description: &'static str,
    pub repository: &'static str,
    pub executable_stem: &'static str,
    pub port: u16,
    pub health_path: &'static str,
    pub management_path: &'static str,
}

pub const COMPONENTS: [ComponentDefinition; 3] = [
    ComponentDefinition {
        id: ComponentId::Cliproxyapi,
        name: "CLIProxyAPI",
        short_name: "CPA Core",
        description: "本地 AI API 网关与协议转换核心",
        repository: "router-for-me/CLIProxyAPI",
        executable_stem: "cli-proxy-api",
        port: 8317,
        health_path: "/healthz",
        management_path: "/management.html",
    },
    ComponentDefinition {
        id: ComponentId::CpaManagerPlus,
        name: "CPA-Manager-Plus",
        short_name: "CPAMP",
        description: "CLIProxyAPI 管理、监控与可视化控制台",
        repository: "seakee/CPA-Manager-Plus",
        executable_stem: "cpa-manager-plus",
        port: 18317,
        health_path: "/usage-service/info",
        management_path: "/management.html",
    },
    ComponentDefinition {
        id: ComponentId::Octopus,
        name: "Octopus",
        short_name: "Octopus",
        description: "面向个人的 LLM API 聚合与负载均衡服务",
        repository: "bestruirui/octopus",
        executable_stem: "octopus",
        port: 8080,
        health_path: "/",
        management_path: "/",
    },
];

pub fn definition(id: ComponentId) -> &'static ComponentDefinition {
    COMPONENTS
        .iter()
        .find(|item| item.id == id)
        .expect("known component")
}

pub fn expected_asset_name(id: ComponentId, version: &str) -> AppResult<String> {
    let version = version.trim_start_matches('v');
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "darwin",
        "linux" => "linux",
        other => return Err(message(format!("暂不支持当前系统：{other}"))),
    };
    let arch = match (id, std::env::consts::ARCH) {
        (ComponentId::Cliproxyapi, "x86_64") => "amd64",
        (ComponentId::Cliproxyapi, "aarch64") => "aarch64",
        (ComponentId::CpaManagerPlus, "x86_64") => "amd64",
        (ComponentId::CpaManagerPlus, "aarch64") => "arm64",
        (ComponentId::Octopus, "x86_64") => "x86_64",
        (ComponentId::Octopus, "aarch64") => "arm64",
        (_, other) => return Err(message(format!("暂不支持当前架构：{other}"))),
    };
    let extension = if os == "windows" { "zip" } else { "tar.gz" };
    Ok(match id {
        ComponentId::Cliproxyapi => format!("CLIProxyAPI_{version}_{os}_{arch}.{extension}"),
        ComponentId::CpaManagerPlus => {
            format!("cpa-manager-plus_v{version}_{os}_{arch}.{extension}")
        }
        ComponentId::Octopus => format!("octopus-{os}-{arch}.zip"),
    })
}

pub fn executable_name(id: ComponentId) -> String {
    let stem = definition(id).executable_stem;
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

pub fn locate_executable(root: &Path, id: ComponentId) -> AppResult<PathBuf> {
    let target = executable_name(id);
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(&target)
            {
                return Ok(path);
            }
        }
    }
    Err(message(format!("解压后未找到预期程序：{target}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_names_exclude_no_plugin_variants() {
        let name = expected_asset_name(ComponentId::Cliproxyapi, "v1.2.3").unwrap();
        assert!(!name.contains("no-plugin"));
        assert!(name.contains("1.2.3"));
    }

    #[test]
    fn octopus_asset_name_matches_upstream_release_convention() {
        let name = expected_asset_name(ComponentId::Octopus, "v0.8.40").unwrap();
        assert!(name.starts_with("octopus-"));
        assert!(name.ends_with(".zip"));
        assert!(!name.contains("0.8.40"));
    }
}
