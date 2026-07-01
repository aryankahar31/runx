use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

pub fn download_to_temp(url: &str) -> Result<PathBuf> {
    println!("Downloading {url}");
    let response = ureq::get(url)
        .set("User-Agent", "runx/0.1.0")
        .call()
        .with_context(|| format!("Failed to download {url}"))?;

    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok());
    let progress = match total {
        Some(bytes) => ProgressBar::new(bytes),
        None => ProgressBar::new_spinner(),
    };
    progress.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {bytes}/{total_bytes} [{bar:40.cyan/blue}] {bytes_per_sec}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );

    let mut temp = NamedTempFile::new().context("Failed to create temporary download file")?;
    let mut reader = response.into_reader();
    copy_with_progress(&mut reader, temp.as_file_mut(), &progress)?;
    progress.finish_and_clear();

    let (_file, path) = temp
        .keep()
        .context("Failed to persist temporary download")?;
    Ok(path)
}

pub fn remove_temp_file(path: &Path) {
    if let Err(err) = std::fs::remove_file(path) {
        eprintln!(
            "Warning: failed to remove temporary file {}: {err}",
            path.display()
        );
    }
}

fn copy_with_progress(
    reader: &mut dyn Read,
    writer: &mut File,
    progress: &ProgressBar,
) -> Result<()> {
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes = reader
            .read(&mut buffer)
            .context("Failed while reading download stream")?;
        if bytes == 0 {
            break;
        }
        writer
            .write_all(&buffer[..bytes])
            .context("Failed while writing download file")?;
        progress.inc(bytes as u64);
    }
    Ok(())
}
