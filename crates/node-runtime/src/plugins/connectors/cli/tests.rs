use super::*;
use serde_json::json;

#[test]
fn rejects_inactive_accounts() {
    let status = CliStatus {
        argv: vec!["status".into()],
        account: "/login".into(),
        condition: Some(sailry_protocol::plugin::model_auth::Condition {
            pointer: "/active".into(),
            values: vec![json!(true)],
        }),
    };
    assert_eq!(
        account(br#"{"login":"fixture-user","active":true}"#, &status).unwrap(),
        "fixture-user"
    );
    assert!(account(br#"{"login":"other-user","active":false}"#, &status).is_err());
    assert!(account(br#"{"login":"$(unsafe)","active":true}"#, &status).is_err());
}

#[cfg(unix)]
mod process {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn executable(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("credential-cli");
        std::fs::write(&path, source).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        (directory, path)
    }
    #[tokio::test]
    async fn preserves_literal_arguments_and_clears_environment() {
        let (_directory, path) =
            executable("#!/bin/sh\nprintf '%s|%s|%s' \"$1\" \"$DECLARED\" \"${GH_TOKEN-unset}\"\n");
        let environment = crate::process::Environment::ambient();
        let output = run(
            &path,
            &["$(touch never); literal".into()],
            &BTreeMap::from([("DECLARED".into(), "public".into())]),
            &environment,
            1000,
            1024,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(output.success);
        assert_eq!(&**output.bytes, b"$(touch never); literal|public|unset");
    }
    #[tokio::test]
    async fn bounds_output_and_execution() {
        let (_directory, path) = executable("#!/bin/sh\nwhile :; do printf 'output'; done\n");
        let environment = crate::process::Environment::ambient();
        assert!(
            run(
                &path,
                &[],
                &BTreeMap::new(),
                &environment,
                1000,
                16,
                &CancellationToken::new()
            )
            .await
            .is_err()
        );
        let (_directory, path) = executable("#!/bin/sh\nwhile :; do :; done\n");
        assert!(
            run(
                &path,
                &[],
                &BTreeMap::new(),
                &environment,
                20,
                1024,
                &CancellationToken::new()
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn cleans_up_descendants() {
        for cancel in [true, false] {
            let (directory, path) = executable(
                "#!/bin/sh\nsleep 60 & task_pid=$!; printf '%s' \"$task_pid\" > \"$1\"; wait \"$task_pid\"\n",
            );
            let pid_file = directory.path().join("child.pid");
            let environment = crate::process::Environment::ambient();
            let stop = CancellationToken::new();
            let argv = [pid_file.to_str().unwrap().into()];
            let env = BTreeMap::new();
            let operation = run(&path, &argv, &env, &environment, 1000, 1024, &stop);
            let observe = async {
                let pid: i32 = tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        if let Ok(text) = std::fs::read_to_string(&pid_file)
                            && let Ok(pid) = text.parse()
                        {
                            break pid;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .expect("credential fixture did not start");
                if cancel {
                    stop.cancel();
                }
                pid
            };
            let (result, pid) = tokio::join!(operation, observe);
            assert_eq!(
                result.err().unwrap().code,
                if cancel {
                    ErrorCode::Cancelled
                } else {
                    ErrorCode::NotConfigured
                }
            );
            tokio::time::timeout(Duration::from_secs(3), async {
                // SAFETY: signal 0 only observes this fixture's exact descendant PID.
                while unsafe { libc::kill(pid, 0) } == 0 {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("credential fixture descendant did not exit");
        }
    }
}
