use super::super::receipt;

#[test]
fn archives_valid_outcome_without_discarding_recovery_information() {
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
        receipt::read(directory.path()).unwrap(),
        Some("updates_completed")
    );
    assert!(!directory.path().join("receipt.json").exists());
    let archived = std::fs::read_dir(directory.path().join("receipts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(
        String::from_utf8(std::fs::read(archived).unwrap())
            .unwrap()
            .contains("isolated-backup")
    );
    assert!(receipt::read(directory.path()).unwrap().is_none());
}

#[test]
fn preserves_invalid_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("receipt.json");
    std::fs::write(&path, b"incompatible user fixture").unwrap();
    assert_eq!(
        receipt::read(directory.path()).unwrap_err().key,
        "updates_receipt_unread"
    );
    assert_eq!(std::fs::read(path).unwrap(), b"incompatible user fixture");
}

#[test]
fn distinguishes_failed_restart_from_an_uncertain_installation() {
    for (field, expected) in [
        ("installed", "updates_restart_failed"),
        ("uncertain", "updates_install_uncertain"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        receipt::write(&directory.path().join("receipt.json"), &serde_json::json!({
            "version": 1, "result": "failure", field: true, "message": "isolated failure details"
        })).unwrap();
        let error = receipt::read(directory.path()).unwrap_err();
        assert_eq!(error.key, expected);
        assert_eq!(error.detail, "isolated failure details");
    }
}

#[test]
fn rejects_unconfirmed_application_version() {
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
        receipt::read(directory.path()).unwrap_err().key,
        "updates_restart_failed"
    );
    assert!(!path.exists());
    let archived = std::fs::read_dir(directory.path().join("receipts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(std::fs::read(archived).unwrap(), bytes);
}

#[test]
fn preserves_invalid_target_version() {
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
            receipt::read(directory.path()).unwrap_err().key,
            "updates_receipt_unread"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
