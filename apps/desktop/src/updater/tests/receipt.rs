use super::super::receipt;
use std::{fs, path::Path};

#[test]
fn outcomes_and_recovery() {
    for (scenario, check) in [
        (
            "archives_valid_outcome_without_discarding_recovery_information",
            (|_scenario: &str| {
                let directory = tempfile::tempdir().unwrap();
                std::fs::write(
                    directory.path().join("receipt.json"),
                    serde_json::to_vec(&serde_json::json!({
                    "version": 1,
                    "result": "success",
                    "application_version": env!("CARGO_PKG_VERSION"),
                    "backup": "isolated-backup"
                        }))
                    .unwrap(),
                )
                .unwrap();
                assert_eq!(
                    receipt::read(directory.path(), None).unwrap(),
                    Some("updates_completed"),
                    "archives valid outcome without discarding recovery information"
                );
                assert!(
                    !directory.path().join("receipt.json").exists(),
                    "archives valid outcome without discarding recovery information"
                );
                let archived = std::fs::read_dir(directory.path().join("receipts"))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path();
                assert!(
                    String::from_utf8(std::fs::read(archived).unwrap())
                        .unwrap()
                        .contains("isolated-backup"),
                    "archives valid outcome without discarding recovery information"
                );
                assert!(
                    receipt::read(directory.path(), None).unwrap().is_none(),
                    "archives valid outcome without discarding recovery information"
                );
            }) as fn(&str),
        ),
        (
            "preserves_invalid_receipt",
            (|_scenario: &str| {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("receipt.json");
                std::fs::write(&path, b"incompatible user fixture").unwrap();
                assert_eq!(
                    receipt::read(directory.path(), None).unwrap_err().key,
                    "updates_receipt_unread",
                    "preserves invalid receipt"
                );
                assert_eq!(
                    std::fs::read(path).unwrap(),
                    b"incompatible user fixture",
                    "preserves invalid receipt"
                );
            }),
        ),
        (
            "distinguishes_failed_restart_from_an_uncertain_installation",
            (|_scenario: &str| {
                for (field, expected) in [
                    ("installed", "updates_restart_failed"),
                    ("uncertain", "updates_install_uncertain"),
                ] {
                    let directory = tempfile::tempdir().unwrap();
                    receipt::write(&directory.path().join("receipt.json"), &serde_json::json!({
            "version": 1, "result": "failure", field: true, "message": "isolated failure details"
                })).unwrap();
                    let error = receipt::read(directory.path(), None).unwrap_err();
                    assert_eq!(
                        error.key, expected,
                        "distinguishes failed restart from an uncertain installation"
                    );
                    assert_eq!(
                        error.detail, "isolated failure details",
                        "distinguishes failed restart from an uncertain installation"
                    );
                }
            }),
        ),
        (
            "rejects_unconfirmed_application_version",
            (|_scenario: &str| {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("receipt.json");
                let bytes = serde_json::to_vec(&serde_json::json!({
                "version": 1,
                "result": "success",
                "application_version": "9.9.9-alpha.2",
                "backup": "isolated-backup"
                    }))
                .unwrap();
                std::fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    receipt::read(directory.path(), None).unwrap_err().key,
                    "updates_restart_failed",
                    "rejects unconfirmed application version"
                );
                assert!(!path.exists(), "rejects unconfirmed application version");
                let archived = std::fs::read_dir(directory.path().join("receipts"))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path();
                assert_eq!(
                    std::fs::read(archived).unwrap(),
                    bytes,
                    "rejects unconfirmed application version"
                );
            }),
        ),
        (
            "preserves_invalid_target_version",
            (|_scenario: &str| {
                for target in [None, Some(""), Some("invalid version")] {
                    let directory = tempfile::tempdir().unwrap();
                    let path = directory.path().join("receipt.json");
                    let mut value = serde_json::json!({"version": 1, "result": "success"});
                    if let Some(target) = target {
                        value["application_version"] = serde_json::json!(target);
                    }
                    let bytes = serde_json::to_vec(&value).unwrap();
                    std::fs::write(&path, &bytes).unwrap();
                    assert_eq!(
                        receipt::read(directory.path(), None).unwrap_err().key,
                        "updates_receipt_unread",
                        "preserves invalid target version"
                    );
                    assert_eq!(
                        std::fs::read(&path).unwrap(),
                        bytes,
                        "preserves invalid target version"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

fn success(directory: &Path, backup: &Path) {
    receipt::write(
        &directory.join("receipt.json"),
        &serde_json::json!({
            "version": 1,
            "result": "success",
            "application_version": env!("CARGO_PKG_VERSION"),
            "backup": backup,
        }),
    )
    .unwrap();
}

#[test]
fn removes_verified_backup() {
    let directory = tempfile::tempdir().unwrap();
    let installed = directory.path().join("Sailry.app");
    let backup = directory.path().join(".sailry-update-backup-previous");
    fs::create_dir(&installed).unwrap();
    fs::create_dir_all(backup.join("Sailry.app")).unwrap();
    fs::write(installed.join("current"), b"running").unwrap();
    fs::write(backup.join("Sailry.app/previous"), b"previous").unwrap();
    success(directory.path(), &backup);

    assert_eq!(
        receipt::read(directory.path(), Some(&installed)).unwrap(),
        Some("updates_completed"),
    );
    assert!(!backup.exists());
    assert_eq!(fs::read(installed.join("current")).unwrap(), b"running");
    assert!(!directory.path().join("receipt.json").exists());
    assert!(
        receipt::read(directory.path(), Some(&installed))
            .unwrap()
            .is_none()
    );
    success(directory.path(), &backup);
    assert!(directory.path().join("receipt.json").is_file());
}

#[test]
fn keeps_unverified_backups() {
    for scenario in ["outside", "prefix", "bundle", "running"] {
        let directory = tempfile::tempdir().unwrap();
        let installed = directory.path().join("Sailry.app");
        fs::create_dir(&installed).unwrap();
        let backup = match scenario {
            "outside" => directory
                .path()
                .join("elsewhere/.sailry-update-backup-previous"),
            "prefix" => directory.path().join("previous"),
            _ => directory.path().join(".sailry-update-backup-previous"),
        };
        fs::create_dir_all(&backup).unwrap();
        if scenario != "bundle" {
            fs::create_dir(backup.join("Sailry.app")).unwrap();
        }
        success(directory.path(), &backup);
        assert_eq!(
            receipt::read(
                directory.path(),
                (scenario != "running").then_some(installed.as_path()),
            )
            .unwrap(),
            Some("updates_completed"),
        );
        assert!(backup.is_dir(), "{scenario}");
    }
}

#[test]
fn keeps_unconfirmed_backups() {
    for (result, uncertain, version, expected) in [
        (
            "failure",
            false,
            env!("CARGO_PKG_VERSION"),
            "updates_install_failed",
        ),
        (
            "failure",
            true,
            env!("CARGO_PKG_VERSION"),
            "updates_install_uncertain",
        ),
        (
            "success",
            true,
            env!("CARGO_PKG_VERSION"),
            "updates_install_uncertain",
        ),
        ("success", false, "9.9.9-alpha.2", "updates_restart_failed"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let installed = directory.path().join("Sailry.app");
        let backup = directory.path().join(".sailry-update-backup-previous");
        fs::create_dir(&installed).unwrap();
        fs::create_dir_all(backup.join("Sailry.app")).unwrap();
        receipt::write(
            &directory.path().join("receipt.json"),
            &serde_json::json!({
                "version": 1,
                "result": result,
                "uncertain": uncertain,
                "application_version": version,
                "backup": backup,
            }),
        )
        .unwrap();
        assert_eq!(
            receipt::read(directory.path(), Some(&installed))
                .unwrap_err()
                .key,
            expected,
        );
        assert!(backup.join("Sailry.app").is_dir());
    }
}

#[cfg(unix)]
#[test]
fn keeps_symlinked_backups() {
    for symlink_bundle in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let installed = directory.path().join("Sailry.app");
        let backup = directory.path().join(".sailry-update-backup-previous");
        fs::create_dir(&installed).unwrap();
        fs::write(installed.join("current"), b"running").unwrap();
        if symlink_bundle {
            fs::create_dir(&backup).unwrap();
            std::os::unix::fs::symlink(&installed, backup.join("Sailry.app")).unwrap();
        } else {
            fs::create_dir(installed.join("Sailry.app")).unwrap();
            std::os::unix::fs::symlink(&installed, &backup).unwrap();
        }
        success(directory.path(), &backup);
        assert_eq!(
            receipt::read(directory.path(), Some(&installed)).unwrap(),
            Some("updates_completed"),
        );
        assert!(backup.exists());
        assert_eq!(fs::read(installed.join("current")).unwrap(), b"running");
    }
}
