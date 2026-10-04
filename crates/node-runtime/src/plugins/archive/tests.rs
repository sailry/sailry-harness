use super::*;
use zip::write::SimpleFileOptions;

fn zip(entries: &[(&str, &[u8])]) -> File {
    let mut zip = zip::ZipWriter::new(tempfile::tempfile().unwrap());
    for (path, bytes) in entries {
        zip.start_file(*path, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap()
}

#[test]
fn rejects_invalid_paths_and_missing_manifest() {
    for path in [
        "../escape",
        "/absolute",
        "C:/drive",
        "folder\\escape",
        "./alias",
    ] {
        assert!(
            unpack(
                zip(&[("plugin.json", b"{}"), (path, b"data")]),
                &CancellationToken::new()
            )
            .is_err(),
            "{path}"
        );
    }
    assert!(
        unpack(
            zip(&[("README.md", b"no manifest")]),
            &CancellationToken::new()
        )
        .is_err()
    );
    assert!(
        unpack(
            zip(&[("one/plugin.json", b"{}"), ("two/plugin.json", b"{}")]),
            &CancellationToken::new()
        )
        .is_err()
    );
}

#[test]
fn rejects_symlinks_and_size_overflow() {
    let mut archive = zip::ZipWriter::new(tempfile::tempfile().unwrap());
    archive
        .add_symlink(
            "plugin.json",
            "/private/secret",
            SimpleFileOptions::default(),
        )
        .unwrap();
    assert!(unpack(archive.finish().unwrap(), &CancellationToken::new()).is_err());
    let mut archive = zip::ZipWriter::new(tempfile::tempfile().unwrap());
    archive
        .start_file("plugin.json", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"{}").unwrap();
    archive
        .add_symlink("CLAUDE.md", "AGENTS.md", SimpleFileOptions::default())
        .unwrap();
    assert!(unpack(archive.finish().unwrap(), &CancellationToken::new()).is_err());
    let mut archive = zip::ZipWriter::new(tempfile::tempfile().unwrap());
    archive
        .start_file("plugin.json", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"{}").unwrap();
    archive
        .start_file("large", SimpleFileOptions::default())
        .unwrap();
    let buffer = [0; 64 * 1024];
    for _ in 0..=MAX_PACKAGE_BYTES / buffer.len() as u64 {
        archive.write_all(&buffer).unwrap();
    }
    assert!(unpack(archive.finish().unwrap(), &CancellationToken::new()).is_err());
}

#[test]
fn cancels_extraction() {
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(
        unpack(zip(&[("plugin.json", b"{}")]), &stop)
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}

#[cfg(unix)]
#[test]
fn preserves_executable_bit() {
    use std::os::unix::fs::PermissionsExt;
    let mut archive = zip::ZipWriter::new(tempfile::tempfile().unwrap());
    archive
        .start_file("plugin.json", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"{}").unwrap();
    archive
        .start_file(
            "bin/tool",
            SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
    archive.write_all(b"executable fixture").unwrap();
    let (_temp, root) = unpack(archive.finish().unwrap(), &CancellationToken::new()).unwrap();
    assert_ne!(
        std::fs::metadata(root.join("bin/tool"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
}
