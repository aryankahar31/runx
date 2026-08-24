//! Archive extraction with containment guarantees.
//!
//! Runtime archives are attacker-relevant input: the download URL is derived
//! from a version string that may come from a checked-in project file, and a
//! compromised or malicious mirror controls the bytes entirely. Extraction
//! therefore treats every entry path and link target as hostile.
//!
//! Three distinct escapes are defended against:
//!
//! 1. **Traversal in the entry path** — `../../etc/cron.d/x`. Rejected by
//!    [`strip_first_component`], which only accepts `Normal` components.
//! 2. **Writing *through* a symlink** — an archive stores a symlink
//!    `bin -> /tmp/evil`, then a regular file `bin/pwned`. Creating the parent
//!    directories of the second entry follows the symlink planted by the first,
//!    so the file lands outside the destination even though neither entry path
//!    contains `..`. Defended by [`ensure_no_symlink_components`], which
//!    refuses to descend through an existing symlink.
//! 3. **Escaping link targets** — a symlink or hard link whose target resolves
//!    outside the destination. Defended by [`link_target_escapes`].
//!
//! Absolute and traversing link targets are skipped rather than fatal: real
//! runtime tarballs contain only relative in-tree links (e.g. Python's
//! `bin/python3 -> python3.11`), so anything else is either junk or an attack,
//! and aborting the whole install would be a denial of service on a
//! well-formed-but-odd archive.

use crate::runtime::ArchiveKind;
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use std::{
    fs::{self, File},
    io,
    path::{Component, Path, PathBuf},
};
use tar::{Archive, EntryType};
use xz2::read::XzDecoder;
use zip::ZipArchive;

pub fn extract_archive(archive: &Path, destination: &Path, kind: ArchiveKind) -> Result<()> {
    extract_archive_with(archive, destination, kind, true)
}

/// Like [`extract_archive`] but keeps the archive's top-level structure.
///
/// Runtime archives all wrap their files in a directory (`bun-darwin-arm64/`,
/// `node-v20.../`), which the default extractor strips. The runx release
/// archives put the binary at the root, so self-update must not strip.
pub fn extract_archive_keep_top_level(
    archive: &Path,
    destination: &Path,
    kind: ArchiveKind,
) -> Result<()> {
    extract_archive_with(archive, destination, kind, false)
}

fn extract_archive_with(
    archive: &Path,
    destination: &Path,
    kind: ArchiveKind,
    strip_first: bool,
) -> Result<()> {
    crate::flags::info(&format!("Extracting to {}", destination.display()));
    match kind {
        ArchiveKind::Zip => extract_zip(archive, destination, strip_first),
        ArchiveKind::TarGz => {
            let file = File::open(archive)
                .with_context(|| format!("Failed to open {}", archive.display()))?;
            extract_tar(GzDecoder::new(file), destination, strip_first)
        }
        ArchiveKind::TarXz => {
            let file = File::open(archive)
                .with_context(|| format!("Failed to open {}", archive.display()))?;
            extract_tar(XzDecoder::new(file), destination, strip_first)
        }
    }
}

fn extract_zip(archive_path: &Path, destination: &Path, strip_first: bool) -> Result<()> {
    let file = File::open(archive_path)
        .with_context(|| format!("Failed to open {}", archive_path.display()))?;
    let mut archive = ZipArchive::new(file).context("Failed to read zip archive")?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("Failed to read zip entry #{index}"))?;

        // `enclosed_name` already rejects absolute paths and `..`.
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let relative = if strip_first {
            // A wrapped archive (every entry under one directory, e.g.
            // `bun-darwin-x64/…`) strips to its inside; an entry at the
            // archive root (a single file, e.g. Deno's executable) has no
            // wrapper to strip and lands in the destination root.
            match strip_first_component(&enclosed).or_else(|| single_component(&enclosed)) {
                Some(relative) => relative,
                None => continue,
            }
        } else {
            enclosed
        };
        let output_path = destination.join(&relative);

        if entry.is_dir() {
            create_dir_all_no_symlinks(destination, &relative)?;
            continue;
        }

        if let Some(parent) = relative.parent() {
            create_dir_all_no_symlinks(destination, parent)?;
        }
        ensure_no_symlink_components(destination, &relative)?;

        let mut output = File::create(&output_path)
            .with_context(|| format!("Failed to create {}", output_path.display()))?;
        io::copy(&mut entry, &mut output)
            .with_context(|| format!("Failed to extract {}", output_path.display()))?;

        // Preserve the executable bit. Zip stores Unix modes in an extra field;
        // without this every extracted binary is 0644 and the runtime cannot be
        // executed. Node ships zips for Windows only, but a zip is perfectly
        // extractable on Unix and silently losing `+x` is a confusing failure.
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&output_path, fs::Permissions::from_mode(mode))
                .with_context(|| format!("Failed to set mode on {}", output_path.display()))?;
        }
    }
    Ok(())
}

fn extract_tar<R: io::Read>(reader: R, destination: &Path, strip_first: bool) -> Result<()> {
    let mut archive = Archive::new(reader);
    let entries = archive.entries().context("Failed to read tar archive")?;

    for entry in entries {
        let mut entry = entry.context("Failed to read tar entry")?;
        let path = entry.path().context("Failed to read tar entry path")?;
        let relative = if strip_first {
            let Some(relative) = strip_first_component(&path) else {
                continue;
            };
            relative
        } else {
            // `strip_first_component` also rejects absolute paths and `..`;
            // without the strip we must apply that sanitisation ourselves,
            // or a crafted archive could escape the destination.
            let mut safe_components = Vec::new();
            for component in path.components() {
                match component {
                    Component::Normal(part) => safe_components.push(part.to_os_string()),
                    Component::CurDir => {}
                    _ => {
                        safe_components.clear();
                        break;
                    }
                }
            }
            if safe_components.is_empty() {
                continue;
            }
            PathBuf::from_iter(safe_components)
        };
        let output_path = destination.join(&relative);
        let entry_type = entry.header().entry_type();

        if entry_type.is_dir() {
            create_dir_all_no_symlinks(destination, &relative)?;
            continue;
        }

        if let Some(parent) = relative.parent() {
            create_dir_all_no_symlinks(destination, parent)?;
        }
        // Re-check after creating parents: an earlier entry may have planted a
        // symlink along this path.
        ensure_no_symlink_components(destination, &relative)?;

        match entry_type {
            EntryType::Symlink | EntryType::Link => {
                let Some(target) = entry.link_name().ok().flatten() else {
                    continue;
                };
                if link_target_escapes(&relative, &target) {
                    eprintln!(
                        "Warning: skipping link {} -> {} (points outside the runtime directory)",
                        relative.display(),
                        target.display()
                    );
                    continue;
                }
                // Remove any existing entry so extraction is idempotent.
                let _ = fs::remove_file(&output_path);
                create_link(entry_type, destination, &relative, &target, &output_path)?;
            }
            // Regular files (and legacy aliases). `unpack` applies the mode
            // from the tar header, which is how the executable bit survives.
            EntryType::Regular | EntryType::Continuous | EntryType::GNUSparse => {
                entry
                    .unpack(&output_path)
                    .with_context(|| format!("Failed to extract {}", output_path.display()))?;
            }
            // Character/block devices, FIFOs, and metadata entries have no
            // place in a runtime tarball.
            _ => continue,
        }
    }
    Ok(())
}

/// Create a symlink or hard link, after its target has been validated.
fn create_link(
    entry_type: EntryType,
    destination: &Path,
    relative: &Path,
    target: &Path,
    output_path: &Path,
) -> Result<()> {
    if entry_type == EntryType::Link {
        // Hard link targets in tar are relative to the archive root, so they
        // need the same leading-component strip as entry paths.
        let Some(target_relative) = strip_first_component(target) else {
            return Ok(());
        };
        let resolved = destination.join(target_relative);
        if !resolved.exists() {
            return Ok(());
        }
        fs::hard_link(&resolved, output_path)
            .or_else(|_| {
                // Cross-device or unsupported: a copy preserves behaviour.
                fs::copy(&resolved, output_path).map(|_| ())
            })
            .with_context(|| format!("Failed to link {}", output_path.display()))?;
        return Ok(());
    }

    symlink_file(target, output_path).with_context(|| {
        format!(
            "Failed to create symlink {} -> {}",
            relative.display(),
            target.display()
        )
    })
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// On Windows, creating a symlink needs Developer Mode or admin rights, so fall
/// back to copying the target. Runtime tarballs use symlinks for aliases like
/// `python3 -> python3.11`, and a copy serves the same purpose.
#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> io::Result<()> {
    if std::os::windows::fs::symlink_file(target, link).is_ok() {
        return Ok(());
    }
    let resolved = link
        .parent()
        .map(|parent| parent.join(target))
        .unwrap_or_else(|| target.to_path_buf());
    if resolved.is_file() {
        fs::copy(&resolved, link).map(|_| ())
    } else {
        // Nothing to alias; leave it absent rather than failing the install.
        Ok(())
    }
}

/// True when `target`, interpreted relative to the directory holding
/// `link_relative`, resolves outside the extraction root.
///
/// Absolute targets always escape. Relative targets are resolved lexically —
/// not with `canonicalize`, which requires existence and would itself follow
/// symlinks.
fn link_target_escapes(link_relative: &Path, target: &Path) -> bool {
    if target.is_absolute() || matches!(target.components().next(), Some(Component::Prefix(_))) {
        return true;
    }

    let mut stack: Vec<std::ffi::OsString> = link_relative
        .parent()
        .map(|parent| {
            parent
                .components()
                .filter_map(|component| match component {
                    Component::Normal(part) => Some(part.to_os_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    for component in target.components() {
        match component {
            Component::Normal(part) => stack.push(part.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                // Popping past the root means the link points outside.
                if stack.pop().is_none() {
                    return true;
                }
            }
            // Root or a drive prefix in a relative target: reject.
            _ => return true,
        }
    }
    false
}

/// Create `destination/relative` and every intermediate directory, refusing to
/// descend through a symlink.
fn create_dir_all_no_symlinks(destination: &Path, relative: &Path) -> Result<()> {
    let mut current = destination.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            continue;
        };
        current.push(part);

        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                anyhow::bail!(
                    "Refusing to extract through the symlink {} (archive may be malicious)",
                    current.display()
                );
            }
            // Already a real directory.
            Ok(metadata) if metadata.is_dir() => {}
            // A file occupies this name; replace it so extraction can continue.
            Ok(_) => {
                fs::remove_file(&current)
                    .with_context(|| format!("Failed to replace {}", current.display()))?;
                fs::create_dir(&current)
                    .with_context(|| format!("Failed to create {}", current.display()))?;
            }
            Err(_) => {
                fs::create_dir(&current)
                    .with_context(|| format!("Failed to create {}", current.display()))?;
            }
        }
    }
    Ok(())
}

/// Verify no component along `relative` is a symlink.
///
/// Called immediately before writing, because a previous entry in the same
/// archive may have planted a symlink after the parent directories were made.
fn ensure_no_symlink_components(destination: &Path, relative: &Path) -> Result<()> {
    let mut current = destination.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    // The final component is the file itself; an existing symlink there is
    // removed by the caller rather than traversed.
    for component in components.iter().take(components.len().saturating_sub(1)) {
        let Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                anyhow::bail!(
                    "Refusing to extract through the symlink {} (archive may be malicious)",
                    current.display()
                );
            }
        }
    }
    Ok(())
}

/// Drop the archive's single wrapping directory and reject unsafe components.
///
/// Runtime tarballs wrap everything in one top-level directory
/// (`node-v20.11.0-linux-x64/`), which is stripped so the cache layout is
/// `<version>/bin/node` rather than `<version>/node-v20.../bin/node`.
///
/// Returns `None` for paths containing `..`, a root, or a drive prefix, and for
/// single-component paths (nothing remains after stripping).
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
        return None;
    }
    Some(safe_components.into_iter().skip(1).collect())
}

/// A single safe path component (e.g. `deno`), or `None` when the path has
/// more than one part or contains anything but normal components.
fn single_component(path: &Path) -> Option<PathBuf> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_os_string()),
            _ => return None,
        }
    }
    match parts.as_slice() {
        [part] => Some(PathBuf::from(part)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    // ── strip_first_component ────────────────────────────────────────────────

    #[test]
    fn strips_the_wrapping_directory() {
        assert_eq!(
            strip_first_component(Path::new("node-v20.11.0-linux-x64/bin/node")),
            Some(PathBuf::from("bin/node"))
        );
    }

    #[test]
    fn rejects_traversal_and_absolute_entry_paths() {
        for bad in [
            "../evil",
            "node-v20/../../evil",
            "/etc/passwd",
            "node-v20/../..",
        ] {
            assert_eq!(
                strip_first_component(Path::new(bad)),
                None,
                "{bad} must be rejected"
            );
        }
    }

    #[test]
    fn returns_none_for_single_component_paths() {
        assert_eq!(strip_first_component(Path::new("README.md")), None);
    }

    // ── zip extraction ───────────────────────────────────────────────────────

    use std::io::Write;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).expect("create zip fixture");
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in entries {
            writer.start_file(*name, options).expect("start entry");
            writer.write_all(content).expect("write entry");
        }
        writer.finish().expect("finish zip");
    }

    /// A root-level single-file archive (Deno's zip contains just the
    /// executable plus small docs at the root) must extract with no wrapper.
    #[test]
    fn extracts_root_level_zip_entries_at_the_destination_root() {
        let dir = tmp();
        let archive = dir.path().join("deno.zip");
        write_zip(
            &archive,
            &[
                ("deno", b"#!/bin/sh\necho deno\n"),
                ("LICENSE.md", b"MIT\n"),
            ],
        );

        let destination = dir.path().join("dest");
        fs::create_dir_all(&destination).unwrap();
        extract_archive(&archive, &destination, ArchiveKind::Zip).expect("extracts");

        assert_eq!(
            fs::read(destination.join("deno")).expect("deno binary present"),
            b"#!/bin/sh\necho deno\n"
        );
        assert!(destination.join("LICENSE.md").exists());
    }

    /// A wrapped archive (Bun) keeps stripping to the destination root.
    #[test]
    fn strips_the_wrapping_directory_of_a_zip_archive() {
        let dir = tmp();
        let archive = dir.path().join("bun.zip");
        write_zip(
            &archive,
            &[
                ("bun-darwin-x64/bun", b"binary"),
                ("bun-darwin-x64/LICENSE.md", b"MIT"),
            ],
        );

        let destination = dir.path().join("dest");
        fs::create_dir_all(&destination).unwrap();
        extract_archive(&archive, &destination, ArchiveKind::Zip).expect("extracts");

        assert!(
            destination.join("bun").exists(),
            "wrapped entry must land at the root"
        );
        assert!(destination.join("LICENSE.md").exists());
    }

    // ── link target validation ───────────────────────────────────────────────

    #[test]
    fn in_tree_relative_links_are_allowed() {
        // Python ships `bin/python3 -> python3.11`.
        assert!(!link_target_escapes(
            Path::new("bin/python3"),
            Path::new("python3.11")
        ));
        assert!(!link_target_escapes(
            Path::new("bin/python3"),
            Path::new("./python3.11")
        ));
        // Climbing and coming back stays inside.
        assert!(!link_target_escapes(
            Path::new("lib/foo/link"),
            Path::new("../bar/target")
        ));
    }

    #[test]
    fn absolute_link_targets_escape() {
        assert!(link_target_escapes(
            Path::new("bin/node"),
            Path::new("/etc/passwd")
        ));
    }

    #[test]
    fn links_climbing_past_the_root_escape() {
        assert!(link_target_escapes(
            Path::new("bin/evil"),
            Path::new("../../../../etc/passwd")
        ));
        // `bin/evil` sits one level deep, so two levels up already escapes.
        assert!(link_target_escapes(
            Path::new("bin/evil"),
            Path::new("../../outside")
        ));
    }

    // ── symlink traversal defence ────────────────────────────────────────────

    /// The confirmed escape: a symlink entry followed by a file entry inside it.
    /// Creating the parent directories must not follow the planted symlink.
    #[test]
    fn refuses_to_create_directories_through_a_symlink() {
        let dir = tmp();
        let destination = dir.path().join("dest");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&destination).unwrap();
        fs::create_dir_all(&outside).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, destination.join("bin")).unwrap();
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(&outside, destination.join("bin")).is_err() {
                return; // Symlink creation needs privileges; nothing to assert.
            }
        }

        let result = create_dir_all_no_symlinks(&destination, Path::new("bin/nested"));
        assert!(
            result.is_err(),
            "must refuse to descend through the symlink"
        );
        assert!(
            !outside.join("nested").exists(),
            "nothing may be created outside the destination"
        );
    }

    #[test]
    fn refuses_to_write_through_a_symlink() {
        let dir = tmp();
        let destination = dir.path().join("dest");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&destination).unwrap();
        fs::create_dir_all(&outside).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, destination.join("bin")).unwrap();
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(&outside, destination.join("bin")).is_err() {
                return;
            }
        }

        assert!(
            ensure_no_symlink_components(&destination, Path::new("bin/pwned")).is_err(),
            "writing through a planted symlink must be refused"
        );
    }

    #[test]
    fn creates_normal_nested_directories() {
        let dir = tmp();
        create_dir_all_no_symlinks(dir.path(), Path::new("lib/python3.11/site-packages"))
            .expect("should create nested dirs");
        assert!(dir.path().join("lib/python3.11/site-packages").is_dir());
    }

    #[test]
    fn replaces_a_file_blocking_a_directory_path() {
        let dir = tmp();
        fs::write(dir.path().join("bin"), b"not a dir").unwrap();
        create_dir_all_no_symlinks(dir.path(), Path::new("bin/node")).expect("should replace file");
        assert!(dir.path().join("bin").is_dir());
    }

    // ── end-to-end archive extraction ────────────────────────────────────────

    /// Build a tar.gz in memory and extract it, asserting containment.
    fn extract_tar_gz(entries: Vec<(tar::Header, Vec<u8>)>, destination: &Path) -> Result<()> {
        let mut builder = tar::Builder::new(Vec::new());
        for (mut header, data) in entries {
            header.set_cksum();
            builder.append(&header, data.as_slice()).unwrap();
        }
        let tar_bytes = builder.into_inner().unwrap();

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        io::Write::write_all(&mut encoder, &tar_bytes).unwrap();
        let gz = encoder.finish().unwrap();

        let archive_path = destination.parent().unwrap().join("test.tar.gz");
        fs::write(&archive_path, gz).unwrap();
        extract_archive(&archive_path, destination, ArchiveKind::TarGz)
    }

    fn file_header(path: &str, size: u64, mode: u32) -> tar::Header {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(size);
        header.set_mode(mode);
        header.set_entry_type(EntryType::Regular);
        header
    }

    #[test]
    fn extracts_a_normal_archive_and_strips_the_wrapper() {
        let dir = tmp();
        let destination = dir.path().join("dest");
        fs::create_dir_all(&destination).unwrap();

        extract_tar_gz(
            vec![(
                file_header("node-v20.11.0/bin/node", 5, 0o755),
                b"hello".to_vec(),
            )],
            &destination,
        )
        .expect("extraction should succeed");

        let extracted = destination.join("bin/node");
        assert_eq!(fs::read(&extracted).unwrap(), b"hello");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&extracted).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "executable bit must be preserved");
        }
    }

    /// The runx release archives (unlike runtime archives) put the binary at
    /// the root; the non-stripping extractor must keep it, while the default
    /// extractor drops root-level files (that is why self-update uses it).
    #[test]
    fn keeps_top_level_files_for_self_update() {
        let dir = tmp();
        let destination = dir.path().join("dest");
        fs::create_dir_all(&destination).unwrap();

        let archive_path = destination.parent().unwrap().join("release.tar.gz");
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = file_header("runx", 3, 0o755);
        header.set_cksum();
        builder.append(&header, b"bin".as_slice()).unwrap();
        let tar_bytes = builder.into_inner().unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        io::Write::write_all(&mut encoder, &tar_bytes).unwrap();
        fs::write(&archive_path, encoder.finish().unwrap()).unwrap();

        extract_archive_keep_top_level(&archive_path, &destination, ArchiveKind::TarGz)
            .expect("extraction should succeed");
        assert_eq!(fs::read(destination.join("runx")).unwrap(), b"bin");
    }

    /// The non-stripping extractor must still reject traversal: an absolute
    /// path must not escape the destination. The `tar` builder validates
    /// paths, so the malicious archive is hand-crafted like one from the wire.
    #[test]
    fn keep_top_level_still_blocks_traversal() {
        let dir = tmp();
        let destination = dir.path().join("dest");
        fs::create_dir_all(&destination).unwrap();

        let mut header = [0u8; 512];
        header[..8].copy_from_slice(b"../evil\0"); // name
        header[100..108].copy_from_slice(b"0000755\0"); // mode
        header[108..124].copy_from_slice(b"0000000000000000"); // uid/gid
        header[124..136].copy_from_slice(b"000000000003"); // size (octal)
        header[136..148].copy_from_slice(b"000000000000"); // mtime
        header[148..156].copy_from_slice(b"        "); // checksum placeholder
        header[156] = b'0'; // typeflag: regular file
        header[257..263].copy_from_slice(b"ustar\0"); // magic
        let checksum: u64 = header.iter().map(|&b| u64::from(b)).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());

        let mut archive = header.to_vec();
        archive.extend_from_slice(b"bin");
        archive.resize(1024, 0); // entry data block + end-of-archive marker

        let archive_path = destination.parent().unwrap().join("evil.tar.gz");
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        io::Write::write_all(&mut encoder, &archive).unwrap();
        fs::write(&archive_path, encoder.finish().unwrap()).unwrap();

        extract_archive_keep_top_level(&archive_path, &destination, ArchiveKind::TarGz)
            .expect("extraction should succeed");
        assert!(!destination.join("evil").exists());
        assert!(!destination.parent().unwrap().join("evil").exists());
    }

    /// The full attack: entry 1 plants a symlink pointing outside, entry 2
    /// writes a file inside it. Nothing may land outside the destination.
    #[test]
    fn archive_cannot_escape_via_planted_symlink() {
        let dir = tmp();
        let destination = dir.path().join("dest");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&destination).unwrap();
        fs::create_dir_all(&outside).unwrap();

        let mut symlink_header = tar::Header::new_gnu();
        symlink_header.set_path("pkg/bin").unwrap();
        symlink_header.set_entry_type(EntryType::Symlink);
        symlink_header.set_size(0);
        symlink_header.set_link_name(&outside).unwrap();

        // Extraction may error or skip; either is acceptable. What matters is
        // that no file escapes.
        let _ = extract_tar_gz(
            vec![
                (symlink_header, Vec::new()),
                (file_header("pkg/bin/pwned", 5, 0o644), b"pwned".to_vec()),
            ],
            &destination,
        );

        assert!(
            !outside.join("pwned").exists(),
            "file escaped the destination via a planted symlink"
        );
    }

    /// Forge a header whose stored name contains `..`.
    ///
    /// `Header::set_path` deliberately refuses traversal paths, so a malicious
    /// archive cannot be produced through the safe API. Writing the name field
    /// directly is the only way to build the input a hostile mirror could serve.
    fn traversal_header(raw_path: &str, size: u64) -> tar::Header {
        let mut header = file_header("placeholder", size, 0o644);
        let name = &mut header.as_gnu_mut().expect("gnu header").name;
        name.fill(0);
        name[..raw_path.len()].copy_from_slice(raw_path.as_bytes());
        header
    }

    #[test]
    fn archive_entries_with_traversal_are_skipped() {
        let dir = tmp();
        let destination = dir.path().join("dest");
        fs::create_dir_all(&destination).unwrap();

        let forged = traversal_header("pkg/../../escaped.txt", 3);
        // Confirm the forged path really does carry the traversal.
        assert!(
            forged
                .path()
                .map(|path| path.to_string_lossy().contains(".."))
                .unwrap_or(false),
            "test setup must produce a traversing entry path"
        );

        let _ = extract_tar_gz(vec![(forged, b"bad".to_vec())], &destination);

        assert!(
            !dir.path().join("escaped.txt").exists(),
            "traversal entry must not be written outside the destination"
        );
        assert!(
            !destination.parent().unwrap().join("escaped.txt").is_file()
                || destination.join("escaped.txt").is_file(),
            "nothing may be written above the destination"
        );
    }

    #[test]
    fn absolute_symlinks_are_skipped_without_failing_the_install() {
        let dir = tmp();
        let destination = dir.path().join("dest");
        fs::create_dir_all(&destination).unwrap();

        let mut evil = tar::Header::new_gnu();
        evil.set_path("pkg/bin/evil").unwrap();
        evil.set_entry_type(EntryType::Symlink);
        evil.set_size(0);
        evil.set_link_name("/etc/passwd").unwrap();

        extract_tar_gz(
            vec![
                (evil, Vec::new()),
                (file_header("pkg/bin/node", 2, 0o755), b"ok".to_vec()),
            ],
            &destination,
        )
        .expect("a bad symlink should not abort the whole extraction");

        assert!(
            !destination.join("bin/evil").exists(),
            "escaping symlink must be skipped"
        );
        assert!(
            destination.join("bin/node").is_file(),
            "the rest of the archive must still extract"
        );
    }

    #[test]
    fn in_tree_symlinks_are_preserved() {
        let dir = tmp();
        let destination = dir.path().join("dest");
        fs::create_dir_all(&destination).unwrap();

        let mut alias = tar::Header::new_gnu();
        alias.set_path("pkg/bin/python3").unwrap();
        alias.set_entry_type(EntryType::Symlink);
        alias.set_size(0);
        alias.set_link_name("python3.11").unwrap();

        extract_tar_gz(
            vec![
                (file_header("pkg/bin/python3.11", 2, 0o755), b"py".to_vec()),
                (alias, Vec::new()),
            ],
            &destination,
        )
        .expect("in-tree symlink should extract");

        let link = destination.join("bin/python3");
        assert!(
            fs::symlink_metadata(&link).is_ok(),
            "the alias should exist (as a symlink or a copy)"
        );
        assert_eq!(fs::read(&link).unwrap(), b"py", "alias should resolve");
    }
}
