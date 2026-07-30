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
    println!("Extracting to {}", destination.display());
    match kind {
        ArchiveKind::Zip => extract_zip(archive, destination),
        ArchiveKind::TarGz => {
            let file = File::open(archive)
                .with_context(|| format!("Failed to open {}", archive.display()))?;
            extract_tar(GzDecoder::new(file), destination)
        }
        ArchiveKind::TarXz => {
            let file = File::open(archive)
                .with_context(|| format!("Failed to open {}", archive.display()))?;
            extract_tar(XzDecoder::new(file), destination)
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

        // `enclosed_name` already rejects absolute paths and `..`.
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let Some(relative) = strip_first_component(&enclosed) else {
            continue;
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

fn extract_tar<R: io::Read>(reader: R, destination: &Path) -> Result<()> {
    let mut archive = Archive::new(reader);
    let entries = archive.entries().context("Failed to read tar archive")?;

    for entry in entries {
        let mut entry = entry.context("Failed to read tar entry")?;
        let path = entry.path().context("Failed to read tar entry path")?;
        let Some(relative) = strip_first_component(&path) else {
            continue;
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
