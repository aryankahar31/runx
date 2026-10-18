#![cfg(unix)]

use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn asset_name() -> String {
    let platform = match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        other => panic!("unsupported test host: {other}"),
    };
    let cpu = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => panic!("unsupported test host: {other}"),
    };
    format!("runx-{platform}-{cpu}.tar.gz")
}

fn release(assets: &Path, version: &str, valid_archive: bool, valid_digest: bool) {
    let name = asset_name();
    let archive = if valid_archive {
        let body = format!("#!/bin/sh\necho 'runx {version}'\n");
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "runx", body.as_bytes())
            .unwrap();
        let tar = builder.into_inner().unwrap();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &tar).unwrap();
        encoder.finish().unwrap()
    } else {
        b"not a gzip archive".to_vec()
    };
    fs::write(assets.join(&name), &archive).unwrap();
    let digest = if valid_digest {
        format!("{:x}", Sha256::digest(&archive))
    } else {
        "0".repeat(64)
    };
    fs::write(assets.join("SHA256SUMS"), format!("{digest}  {name}\n")).unwrap();
}

fn run_installer(root: &Path, version: &str, fail_download: bool) -> std::process::Output {
    let assets = root.join("assets");
    let fake_bin = root.join("fake-bin");
    let install_dir = root.join("installed");
    fs::create_dir_all(&fake_bin).unwrap();
    fs::write(
        fake_bin.join("curl"),
        "#!/bin/sh\nif [ \"${RUNX_TEST_CURL_FAIL:-0}\" = 1 ]; then exit 22; fi\n\
         out=; url=\nwhile [ \"$#\" -gt 0 ]; do\n  case \"$1\" in\n    -o) out=$2; shift 2 ;;\n    https://*) url=$1; shift ;;\n    *) shift ;;\n  esac\ndone\n\
         cp \"$RUNX_TEST_ASSETS/$(basename \"$url\")\" \"$out\"\n",
    )
    .unwrap();
    fs::set_permissions(fake_bin.join("curl"), fs::Permissions::from_mode(0o755)).unwrap();

    Command::new("sh")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/install.sh"))
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("RUNX_TEST_ASSETS", assets)
        .env("RUNX_TEST_CURL_FAIL", if fail_download { "1" } else { "0" })
        .env("RUNX_INSTALL_DIR", install_dir)
        .env("RUNX_INSTALL_NO_MODIFY_PATH", "1")
        .env("RUNX_VERSION", format!("v{version}"))
        .output()
        .unwrap()
}

#[test]
fn installer_stages_fresh_install_and_preserves_upgrade_on_failures() {
    let root = tempfile::tempdir().unwrap();
    let assets = root.path().join("assets");
    fs::create_dir_all(&assets).unwrap();
    let installed = root.path().join("installed/runx");

    release(&assets, "0.6.0", true, true);
    let fresh = run_installer(root.path(), "0.6.0", false);
    assert!(
        fresh.status.success(),
        "{}",
        String::from_utf8_lossy(&fresh.stderr)
    );
    assert_eq!(
        Command::new(&installed)
            .arg("--version")
            .output()
            .unwrap()
            .stdout,
        b"runx 0.6.0\n"
    );

    release(&assets, "0.6.1", true, false);
    assert!(!run_installer(root.path(), "0.6.1", false).status.success());
    release(&assets, "0.6.1", false, true);
    assert!(!run_installer(root.path(), "0.6.1", false).status.success());
    release(&assets, "9.9.9", true, true);
    assert!(!run_installer(root.path(), "0.6.1", false).status.success());
    assert!(!run_installer(root.path(), "0.6.1", true).status.success());
    assert_eq!(
        Command::new(&installed)
            .arg("--version")
            .output()
            .unwrap()
            .stdout,
        b"runx 0.6.0\n"
    );

    release(&assets, "0.6.1", true, true);
    let upgrade = run_installer(root.path(), "0.6.1", false);
    assert!(
        upgrade.status.success(),
        "{}",
        String::from_utf8_lossy(&upgrade.stderr)
    );
    assert_eq!(
        Command::new(&installed)
            .arg("--version")
            .output()
            .unwrap()
            .stdout,
        b"runx 0.6.1\n"
    );
    let leftovers: Vec<_> = fs::read_dir(root.path().join("installed"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".runx-install")
        })
        .collect();
    assert!(leftovers.is_empty(), "staged candidates must be cleaned up");
}
