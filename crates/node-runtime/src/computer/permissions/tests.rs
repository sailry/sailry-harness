use super::*;

fn envelope(accessibility: serde_json::Value, screen: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "version": "2.6",
        "ok": true,
        "command": "permissions",
        "data": {
            "accessibility": accessibility,
            "automation": {"state": "unknown"},
            "screen_recording": screen
        }
    })
}

#[test]
fn maps_reported_grants() {
    use serde_json::json;
    assert_eq!(DEADLINE, std::time::Duration::from_secs(15));
    let denied = json!({"state": "denied", "suggestion": "Run agent-desktop permissions --guide"});
    for (state, expected) in [
        (json!({"state": "granted"}), Some(true)),
        (denied, Some(false)),
        (json!({"state": "not_required"}), None),
        (json!({"state": "unknown"}), None),
    ] {
        let bytes =
            serde_json::to_vec(&envelope(state.clone(), json!({"state": "granted"}))).unwrap();
        assert_eq!(
            parse(&bytes).unwrap(),
            Grants {
                screen_capture: Some(true),
                accessibility: expected
            }
        );
        let bytes = serde_json::to_vec(&envelope(json!({"state": "granted"}), state)).unwrap();
        assert_eq!(
            parse(&bytes).unwrap(),
            Grants {
                screen_capture: expected,
                accessibility: Some(true)
            }
        );
    }
    let granted = envelope(json!({"state": "granted"}), json!({"state": "granted"}));
    let failed = json!({
        "version": "2.6",
        "ok": false,
        "command": "permissions",
        "error": {"code": "INTERNAL", "message": "failed", "disposition": {"delivery": "not_delivered", "retry": "safe"}}
    });
    let mut other = granted.clone();
    other["command"] = "status".into();
    let mut missing = granted.clone();
    missing["data"]
        .as_object_mut()
        .unwrap()
        .remove("screen_recording");
    let mut renamed = granted.clone();
    renamed["data"]["accessibility"]["state"] = "trusted".into();
    let mut empty = granted;
    empty["data"]["accessibility"] = serde_json::Value::Null;
    for malformed in [failed, other, missing, renamed, empty] {
        let bytes = serde_json::to_vec(&malformed).unwrap();
        assert_eq!(parse(&bytes).unwrap_err().code, ErrorCode::Unavailable);
    }
    assert_eq!(parse(b"not json").unwrap_err().code, ErrorCode::Unavailable);
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
                ("AGENT_DESKTOP_HOME".into(), "owned state".into()),
                ("SAILRY_FIXTURE".into(), "literal ${HOME}/value".into()),
            ]),
            cwd: root,
            roots: (handle.try_clone().unwrap(), handle),
            settings: None,
            python: None,
            computer: true,
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
            "printf '%s|%s|%s|%s' \"$*\" \"$AGENT_DESKTOP_HOME\" \"$SAILRY_FIXTURE\" \"$PWD\"",
        );
        let Launch::Stdio { cwd, .. } = &launch else {
            unreachable!()
        };
        for request in [false, true] {
            let bytes = run(
                &launch,
                &Environment::ambient(),
                request,
                &CancellationToken::new(),
                DEADLINE,
            )
            .await
            .unwrap();
            assert_eq!(
                String::from_utf8(bytes).unwrap(),
                format!(
                    "{}|owned state|literal ${{HOME}}/value|{}",
                    if request {
                        "permissions --request"
                    } else {
                        "permissions"
                    },
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
