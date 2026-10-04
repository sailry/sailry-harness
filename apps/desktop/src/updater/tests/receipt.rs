use super::super::receipt;

#[test]
fn archives_valid_outcome_without_discarding_recovery_information() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("receipt.json"),
        br#"{"version":1,"result":"success","backup":"isolated-backup"}"#,
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
