#![cfg(not(unix))]

#[test]
fn management_does_not_open_a_profile() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("never started");
    for task in ["submit", "wait"] {
        let binary = std::env::var_os("SAILRY_TEST_HOST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_sailry-host").into());
        let output = std::process::Command::new(binary)
            .arg(task)
            .arg("--data-dir")
            .arg(&profile)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("requires a Unix Host control socket")
        );
        assert!(!profile.exists());
    }
}
