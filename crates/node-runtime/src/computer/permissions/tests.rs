use super::*;

#[test]
fn maps_reported_grants() {
    assert_eq!(DEADLINE, std::time::Duration::from_secs(15));
    for screen in [false, true] {
        for accessibility in [false, true] {
            let state = parse(
                &serde_json::to_vec(&serde_json::json!({
                    "screenCaptureGranted": screen,
                    "accessibilityTrusted": accessibility,
                    "otherDiagnostic": "ignored"
                }))
                .unwrap(),
            )
            .unwrap();
            assert_eq!(state.screen_capture, screen);
            assert_eq!(state.accessibility, accessibility);
        }
    }
    for malformed in [
        b"not json".as_slice(),
        br#"{"screenCaptureGranted":false}"#,
        br#"{"screenCaptureGranted":false,"accessibilityTrusted":null}"#,
    ] {
        assert_eq!(parse(malformed).err().unwrap().code, ErrorCode::Unavailable);
    }
}

#[tokio::test]
async fn confines_permission_setup() {
    let node = NodeId([1; 32]);
    let caller = NodeId([2; 32]);
    let environment = Environment::ambient();
    let stop = CancellationToken::new();
    assert_eq!(
        request(
            node,
            caller,
            Permission::Accessibility,
            None,
            &environment,
            &stop
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    let state = read(node, node, None, &environment, &stop).await;
    #[cfg(target_os = "macos")]
    assert_eq!(state.unwrap_err().code, ErrorCode::NotConfigured);
    #[cfg(not(target_os = "macos"))]
    {
        let state = state.unwrap();
        assert_eq!(state.platform, std::env::consts::OS);
        assert!(state.local);
        assert_eq!(state.screen_capture, None);
        assert_eq!(state.accessibility, None);
        assert_eq!(
            request(
                node,
                node,
                Permission::Accessibility,
                None,
                &environment,
                &stop
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::Unavailable
        );
    }
}

#[cfg(unix)]
mod process {
    use super::*;
    use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, path::Path, time::Duration};

    fn fixture(body: &str) -> (tempfile::TempDir, Launch) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let path = root.join("doctor");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let handle =
            cap_std::fs::Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let launch = Launch::Stdio {
            command: path,
            args: vec!["mcp".into()],
            env: BTreeMap::from([
                (
                    "OPEN_COMPUTER_USE_AGENT_SOCKET_NAMESPACE".into(),
                    "owned namespace".into(),
                ),
                (
                    "OPEN_COMPUTER_USE_CONFIG_FILE".into(),
                    "literal ${HOME}/ocu.json".into(),
                ),
            ]),
            cwd: root,
            roots: (handle.try_clone().unwrap(), handle),
            settings: None,
            python: None,
        };
        (directory, launch)
    }

    fn reaped(directory: &Path) {
        let pid: i32 = std::fs::read_to_string(directory.join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let mut status = 0;
        // SAFETY: this probes only the fixture's child PID and never sends a signal.
        assert_eq!(
            unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }

    #[tokio::test]
    async fn preserves_captured_configuration() {
        let (_directory, launch) = fixture(
            "printf '%s|%s|%s|%s' \"$*\" \"$OPEN_COMPUTER_USE_AGENT_SOCKET_NAMESPACE\" \"$OPEN_COMPUTER_USE_CONFIG_FILE\" \"$PWD\"",
        );
        let Launch::Stdio { cwd, .. } = &launch else {
            unreachable!()
        };
        for json in [false, true] {
            let bytes = run(
                &launch,
                &Environment::ambient(),
                json,
                &CancellationToken::new(),
                DEADLINE,
            )
            .await
            .unwrap();
            assert_eq!(
                String::from_utf8(bytes).unwrap(),
                format!(
                    "{}|owned namespace|literal ${{HOME}}/ocu.json|{}",
                    if json { "doctor --json" } else { "doctor" },
                    cwd.display()
                )
            );
        }
    }

    #[tokio::test]
    async fn reports_launch_and_exit_failures() {
        let (_directory, launch) = fixture("printf 'private credential' >&2\nexit 17");
        let error = run(
            &launch,
            &Environment::ambient(),
            true,
            &CancellationToken::new(),
            DEADLINE,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(!error.message.contains("private credential"));
        let Launch::Stdio { command, .. } = &launch else {
            unreachable!()
        };
        std::fs::remove_file(command).unwrap();
        assert_eq!(
            run(
                &launch,
                &Environment::ambient(),
                true,
                &CancellationToken::new(),
                DEADLINE
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::Unavailable
        );
    }

    #[tokio::test]
    async fn bounds_output_and_reaps() {
        let (directory, launch) =
            fixture("printf '%s' $$ > pid\nwhile :; do printf 'diagnostic'; done");
        let error = run(
            &launch,
            &Environment::ambient(),
            true,
            &CancellationToken::new(),
            DEADLINE,
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("output limit"));
        reaped(directory.path());
    }

    #[tokio::test]
    async fn bounds_waits_and_reaps() {
        let (directory, launch) = fixture("printf '%s' $$ > pid\nwhile :; do :; done");
        let error = run(
            &launch,
            &Environment::ambient(),
            true,
            &CancellationToken::new(),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("timed out"));
        reaped(directory.path());
    }

    #[tokio::test]
    async fn cancels_and_reaps() {
        let (directory, launch) = fixture("printf '%s' $$ > pid\nwhile :; do :; done");
        let stop = CancellationToken::new();
        let closed = stop.clone();
        let operation = tokio::spawn(async move {
            run(&launch, &Environment::ambient(), true, &closed, DEADLINE).await
        });
        tokio::time::timeout(DEADLINE, async {
            while std::fs::read_to_string(directory.path().join("pid"))
                .ok()
                .and_then(|pid| pid.parse::<i32>().ok())
                .is_none()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        stop.cancel();
        assert_eq!(
            operation.await.unwrap().unwrap_err().code,
            ErrorCode::Cancelled
        );
        reaped(directory.path());
    }
}
