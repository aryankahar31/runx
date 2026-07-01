use crate::error::UserError;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{env, path::PathBuf, thread, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
    TarXz,
}

#[derive(Debug, Clone)]
pub struct RuntimeSpec {
    pub tool: String,
    pub version: String,
    pub url: String,
    pub archive_kind: ArchiveKind,
    pub executable: String,
    pub bin_dirs: Vec<PathBuf>,
}

pub fn resolve_runtime(tool: &str, version: &str) -> Result<RuntimeSpec> {
    match normalized_tool(tool).as_str() {
        "node" => resolve_node(version),
        "python" => resolve_python(version),
        _ => Err(UserError::new(format!(
            "Unsupported runtime `{tool}`. Supported runtimes: node, python."
        ))
        .into()),
    }
}

fn normalized_tool(tool: &str) -> String {
    tool.trim().to_ascii_lowercase()
}

fn resolve_node(version: &str) -> Result<RuntimeSpec> {
    let platform = match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => ("linux-x64", ArchiveKind::TarXz),
        ("linux", "aarch64") => ("linux-arm64", ArchiveKind::TarXz),
        ("macos", "x86_64") => ("darwin-x64", ArchiveKind::TarGz),
        ("macos", "aarch64") => ("darwin-arm64", ArchiveKind::TarGz),
        ("windows", "x86_64") => ("win-x64", ArchiveKind::Zip),
        ("windows", "aarch64") => ("win-arm64", ArchiveKind::Zip),
        (os, arch) => {
            return Err(
                UserError::new(format!("Node runtime is not supported on {os}/{arch}.")).into(),
            )
        }
    };

    let ext = match platform.1 {
        ArchiveKind::Zip => "zip",
        ArchiveKind::TarGz => "tar.gz",
        ArchiveKind::TarXz => "tar.xz",
    };
    let url = format!(
        "https://nodejs.org/dist/v{version}/node-v{version}-{}.{}",
        platform.0, ext
    );

    Ok(RuntimeSpec {
        tool: "node".to_string(),
        version: version.to_string(),
        url,
        archive_kind: platform.1,
        executable: executable_name("node"),
        bin_dirs: node_bin_dirs(),
    })
}

fn resolve_python(version: &str) -> Result<RuntimeSpec> {
    let platform = match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        (os, arch) => {
            return Err(
                UserError::new(format!("Python runtime is not supported on {os}/{arch}.")).into(),
            )
        }
    };

    let asset = find_python_asset(version, platform)?;
    Ok(RuntimeSpec {
        tool: "python".to_string(),
        version: version.to_string(),
        url: asset.browser_download_url,
        archive_kind: ArchiveKind::TarGz,
        executable: executable_name("python"),
        bin_dirs: python_bin_dirs(),
    })
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

fn find_python_asset(version: &str, platform: &str) -> Result<GithubAsset> {
    let prefix = format!("cpython-{version}+");
    for page in 1..=20 {
        let url = format!(
            "https://api.github.com/repos/astral-sh/python-build-standalone/releases?per_page=10&page={page}"
        );
        let releases = fetch_python_release_page(&url)?;

        if releases.is_empty() {
            break;
        }

        if let Some(asset) = releases
            .into_iter()
            .flat_map(|release| release.assets)
            .find(|asset| {
                asset.name.starts_with(&prefix)
                    && asset.name.contains(platform)
                    && asset.name.contains("install_only")
                    && asset.name.ends_with(".tar.gz")
            })
        {
            return Ok(asset);
        }
    }

    Err(UserError::new(format!(
        "No portable Python {version} archive found for {platform} in python-build-standalone releases."
    ))
    .into())
}

fn fetch_python_release_page(url: &str) -> Result<Vec<GithubRelease>> {
    let mut last_error = None;
    for attempt in 1..=3 {
        match ureq::get(url).set("User-Agent", "runx/0.1.0").call() {
            Ok(response) => {
                return response
                    .into_json()
                    .with_context(|| "Failed to decode python-build-standalone release metadata");
            }
            Err(err) => {
                last_error = Some(err);
                if attempt < 3 {
                    thread::sleep(Duration::from_secs(attempt));
                }
            }
        }
    }

    let err = last_error
        .map(|err| err.to_string())
        .unwrap_or_else(|| "unknown error".to_string());
    Err(anyhow::anyhow!(
        "Failed to query python-build-standalone releases at {url}: {err}"
    ))
}

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn node_bin_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![PathBuf::from(".")]
    } else {
        vec![PathBuf::from("bin")]
    }
}

fn python_bin_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![PathBuf::from("."), PathBuf::from("Scripts")]
    } else {
        vec![PathBuf::from("bin")]
    }
}
