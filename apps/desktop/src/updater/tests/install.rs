use super::super::install;
#[cfg(target_os = "macos")]
use super::fixture;

#[test]
fn rejects_profiles_and_recovery_inside_app() {
    let directory = tempfile::tempdir().unwrap();
    let installed = directory.path().join("Sailry.app");
    let profile = directory.path().join("profile");
    let nested = installed.join("profile");
    for path in [&profile, &nested] {
        std::fs::create_dir_all(path).unwrap();
    }
    std::fs::write(nested.join("sentinel"), b"user data").unwrap();
    assert!(install::separate_data(&installed, &profile, &profile.join("desktop/updater")).is_ok());
    assert_eq!(
        install::separate_data(&installed, &nested, &profile.join("desktop/updater"))
            .unwrap_err()
            .key,
        "updates_profile_inside"
    );
    assert_eq!(
        install::separate_data(&installed, &profile, &nested.join("desktop/updater"))
            .unwrap_err()
            .key,
        "updates_profile_inside"
    );
    #[cfg(unix)]
    {
        let link = profile.join("desktop");
        std::os::unix::fs::symlink(&nested, &link).unwrap();
        assert_eq!(
            install::separate_data(&installed, &profile, &link.join("updater"))
                .unwrap_err()
                .key,
            "updates_profile_inside"
        );
    }
    assert_eq!(
        std::fs::read(nested.join("sentinel")).unwrap(),
        b"user data"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn swaps_whole_bundle_and_retains_independent_original() {
    let staged = fixture::staged();
    let directory = tempfile::tempdir().unwrap();
    let installed = directory.path().join("Sailry.app");
    let mut old = staged.selection.release.clone();
    old.version = "0.1.0".into();
    fixture::bundle(&installed, &old);
    std::fs::write(
        installed.join("Contents/Resources/original-only"),
        b"original resource",
    )
    .unwrap();
    let backup = install::replace(&staged, &fixture::config(), &installed).unwrap();
    assert!(
        installed
            .join("Contents/Resources/office-runtime/python/bin/python3.12")
            .is_file()
    );
    assert!(!installed.join("Contents/Resources/original-only").exists());
    assert_eq!(
        std::fs::read(backup.join("Sailry.app/Contents/Resources/original-only")).unwrap(),
        b"original resource"
    );
    assert!(
        backup
            .join("Sailry.app/Contents/MacOS/sailry-desktop")
            .is_file()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn invalid_signature_leaves_installed_bundle_untouched() {
    let mut staged = fixture::staged();
    let directory = tempfile::tempdir().unwrap();
    let installed = directory.path().join("Sailry.app");
    std::fs::create_dir(&installed).unwrap();
    std::fs::write(installed.join("sentinel"), b"untouched").unwrap();
    staged.selection.release.sha256 = "0".repeat(64);
    assert!(install::replace(&staged, &fixture::config(), &installed).is_err());
    assert_eq!(
        std::fs::read(installed.join("sentinel")).unwrap(),
        b"untouched"
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}
