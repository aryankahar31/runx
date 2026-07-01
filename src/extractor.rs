use crate::runtime::ArchiveKind;
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::{
    fs::{self, File},
    io,
    path::{Component, Path, PathBuf},
};
use tar::Archive;
use xz2::read::XzDecoder;
use zip::ZipArchive;

pub fn extract_archive(archive: &Path, destination: &Path, kind: ArchiveKind) -> Result<()> {
    println!("Extracting to {}", destination.display());
    match kind {
        ArchiveKind::Zip => extract_zip(archive, destination),
        ArchiveKind::TarGz => {
            let file = File::open(archive)
                .with_context(|| format!("Failed to open {}", archive.display()))?;
            let decoder = GzDecoder::new(file);
            extract_tar(decoder, destination)
        }
        ArchiveKind::TarXz => {
            let file = File::open(archive)
                .with_context(|| format!("Failed to open {}", archive.display()))?;
            let decoder = XzDecoder::new(file);
            extract_tar(decoder, destination)
        }
    }
}

fn extract_zip(archive_path: &Path, destination: &Path) -> Result<()> {
    let file = File::open(archive_path)
        .with_context(|| format!("Failed to open {}", archive_path.display()))?;
    let mut archive = ZipArchive::new(file).context("Failed to read zip archive")?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("Failed to read zip entry #{index}"))?;
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let Some(relative) = strip_first_component(&enclosed) else {
            continue;
        };
        let output_path = destination.join(relative);

        if entry.is_dir() {
            fs::create_dir_all(&output_path)
                .with_context(|| format!("Failed to create {}", output_path.display()))?;
        } else {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            let mut output = File::create(&output_path)
                .with_context(|| format!("Failed to create {}", output_path.display()))?;
            io::copy(&mut entry, &mut output)
                .with_context(|| format!("Failed to extract {}", output_path.display()))?;
        }
    }
    Ok(())
}

fn extract_tar<R: io::Read>(reader: R, destination: &Path) -> Result<()> {
    let mut archive = Archive::new(reader);
    let entries = archive.entries().context("Failed to read tar archive")?;

    for entry in entries {
        let mut entry = entry.context("Failed to read tar entry")?;
        let path = entry.path().context("Failed to read tar entry path")?;
        let Some(relative) = strip_first_component(&path) else {
            continue;
        };
        let output_path = destination.join(relative);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        entry
            .unpack(&output_path)
            .with_context(|| format!("Failed to extract {}", output_path.display()))?;
    }
    Ok(())
}

fn strip_first_component(path: &Path) -> Option<PathBuf> {
    let mut safe_components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => safe_components.push(part.to_os_string()),
            Component::CurDir => {}
            _ => return None,
        }
    }

    if safe_components.len() <= 1 {
        None
    } else {
        let mut stripped = PathBuf::new();
        for component in safe_components.into_iter().skip(1) {
            stripped.push(component);
        }
        Some(stripped)
    }
}
