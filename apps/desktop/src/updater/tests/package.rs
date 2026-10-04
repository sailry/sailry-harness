use super::{super::package, fixture};
use std::fs;

#[test]
fn verifies_publisher_and_archive_integrity() {
    let directory = tempfile::tempdir().unwrap();
    let mut release = fixture::release();
    let bytes = fixture::archive(&[("Sailry.app/file", b"isolated fixture")], &release.name);
    let path = directory.path().join(&release.name);
    fs::write(&path, &bytes).unwrap();
    release.size = bytes.len() as u64;
    release.sha256 = fixture::digest(&bytes);
    package::verify_archive(&path, &release, &fixture::config().keys).unwrap();
    assert!(package::verify_archive(&path, &release, &[]).is_err());
    release.sha256 = "0".repeat(64);
    assert_eq!(
        package::verify_archive(&path, &release, &fixture::config().keys)
            .unwrap_err()
            .key,
        "updates_digest_invalid"
    );
    let mut changed = bytes;
    let last = changed.len() - 1;
    changed[last] ^= 1;
    release.sha256 = fixture::digest(&changed);
    fs::write(&path, changed).unwrap();
    assert_eq!(
        package::verify_archive(&path, &release, &fixture::config().keys)
            .unwrap_err()
            .key,
        "updates_signature_invalid"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn extracts_complete_signed_bundle() {
    let staged = fixture::staged();
    let destination = staged.directory.path().join("extracted");
    fs::create_dir(&destination).unwrap();
    let root = package::extract(&staged.archive, &staged.selection.release, &destination).unwrap();
    assert!(
        root.join("Contents/Resources/office-runtime/python/bin/python3.12")
            .is_file()
    );
    assert!(
        root.join("Contents/Resources/hosts/aarch64-unknown-linux-gnu/sailry-host")
            .is_file()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn rejects_missing_runtime_or_wrong_architecture() {
    let staged = fixture::staged();
    let root = staged.directory.path().join("Sailry.app");
    let runtime = root.join("Contents/Resources/office-runtime/runtime.json");
    fs::rename(&runtime, runtime.with_extension("saved")).unwrap();
    assert!(package::validate(&root, &staged.selection.release).is_err());
    fs::rename(runtime.with_extension("saved"), &runtime).unwrap();
    let mut wrong = staged.selection.release.clone();
    wrong.target = if wrong.target.starts_with("aarch64") {
        "x86_64-apple-darwin"
    } else {
        "aarch64-apple-darwin"
    }
    .into();
    assert_eq!(
        package::validate(&root, &wrong).unwrap_err().key,
        "updates_platform"
    );
}
