#![cfg(not(unix))]

#[test]
fn reports_unsupported_without_creating_profile() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("absent");
    let binary = std::env::var_os("SAILRY_TEST_HOST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_sailry-host").into());
    let output = std::process::Command::new(binary)
        .arg("acp")
        .arg("--data-dir")
        .arg(&profile)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("ACP management requires a Unix Host control socket")
    );
    assert!(!profile.exists());
}
