use super::*;

fn launch(root: &Path, command: &str) -> Launch {
    Launch {
        root: root.canonicalize().unwrap(),
        command: command.into(),
        cwd: String::new(),
        timeout_ms: 5000,
        inputs: None,
    }
}

#[test]
fn rejects_invalid_inputs() {
    let directory = tempfile::tempdir().unwrap();
    for command in ["", " \n", "echo\0invalid"] {
        assert!(launch(directory.path(), command).validate().is_err());
    }
    for cwd in ["bad\0path", "bad\npath", "bad\u{7f}path"] {
        let mut request = launch(directory.path(), "echo test");
        request.cwd = cwd.into();
        assert!(request.validate().is_err());
    }
    for timeout in [0, 900_001] {
        let mut request = launch(directory.path(), "echo test");
        request.timeout_ms = timeout;
        assert!(request.validate().is_err());
    }
}

#[test]
fn resolves_native_working_directories() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let nested = root.join("资料 with spaces");
    let sibling = directory.path().join("skill files");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir(&sibling).unwrap();
    for (cwd, expected) in [
        (String::new(), root.clone()),
        (".".into(), root.clone()),
        ("./资料 with spaces/".into(), nested.clone()),
        ("资料 with spaces/..".into(), root.clone()),
        ("../skill files".into(), sibling.clone()),
        (nested.to_str().unwrap().into(), nested.clone()),
        (sibling.to_str().unwrap().into(), sibling.clone()),
    ] {
        let mut request = launch(&root, "echo test");
        request.cwd = cwd;
        assert_eq!(
            request.directory().unwrap(),
            expected.canonicalize().unwrap()
        );
    }
    let file = root.join("file.txt");
    std::fs::write(&file, "content").unwrap();
    for cwd in ["missing", "file.txt"] {
        let mut request = launch(&root, "echo test");
        request.cwd = cwd.into();
        assert_eq!(
            request.directory().unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}

#[cfg(unix)]
mod unix {
    use super::*;

    async fn run(launch: Launch) -> Completion {
        execute(
            launch,
            &Environment::capture(),
            &CancellationToken::new(),
            &CancellationToken::new(),
        )
        .await
        .unwrap()
    }

    #[test]
    fn excludes_ambient_credentials() {
        if std::env::var_os("SAILRY_COMMAND_ENV_TEST").is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "process::tests::unix::excludes_ambient_credentials",
                    "--nocapture",
                ])
                .env("SAILRY_COMMAND_ENV_TEST", "1")
                .env("SAILRY_FIXTURE_SECRET", "isolated-test-value")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let output = runtime.block_on(run(launch(
            directory.path(),
            "printf '%s' \"${SAILRY_FIXTURE_SECRET-unset}\"",
        )));
        assert_eq!(output.outcome, Outcome::Exited(0));
        assert_eq!(output.stdout.text, "unset");
    }

    async fn gone(pid: i32) {
        tokio::time::timeout(Duration::from_secs(3), async {
            // Read-only probe of the exact fixture child, never a global process search.
            while unsafe { libc::kill(pid, 0) } == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture descendant did not exit");
    }

    async fn pid(path: &Path) -> i32 {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(text) = std::fs::read_to_string(path)
                    && let Ok(pid) = text.parse()
                {
                    return pid;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture did not start")
    }

    #[tokio::test]
    async fn captures_unicode_and_exit_status() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("项目")).unwrap();
        let mut request = launch(
            directory.path(),
            "printf '中文 🙂'; printf 'stderr text' >&2; exit 7",
        );
        request.cwd = "项目".into();
        let output = run(request).await;
        assert_eq!(output.outcome, Outcome::Exited(7));
        assert_eq!(output.stdout.text, "中文 🙂");
        assert_eq!(output.stderr.text, "stderr text");
        assert!(!output.stdout.truncated);
        let mut request = launch(directory.path(), "pwd");
        request.cwd = "项目".into();
        assert_eq!(
            run(request).await.stdout.text.trim(),
            directory
                .path()
                .join("项目")
                .canonicalize()
                .unwrap()
                .to_str()
                .unwrap()
        );
    }

    #[tokio::test]
    async fn drains_truncated_output() {
        let directory = tempfile::tempdir().unwrap();
        let output = run(launch(directory.path(), "dd if=/dev/zero bs=1024 count=80 2>/dev/null; { dd if=/dev/zero bs=1024 count=80 2>/dev/null; } >&2")).await;
        assert_eq!(output.outcome, Outcome::Exited(0));
        assert!(output.stdout.truncated);
        assert_eq!(output.stdout.text.len(), 64 * 1024);
        assert!(output.stderr.truncated);
        assert_eq!(output.stderr.text.len(), 64 * 1024);
        assert!(serde_json::to_vec(&output).unwrap().len() < sailry_protocol::MAX_FRAME_BYTES);
        let output = run(launch(directory.path(), "printf '\\377'")).await;
        assert!(output.stdout.invalid_utf8);
    }

    #[tokio::test]
    async fn timeout_stops_descendants() {
        let directory = tempfile::tempdir().unwrap();
        let mut request = launch(
            directory.path(),
            "sleep 60 & task_pid=$!; printf '%s' \"$task_pid\" > child.pid; printf 'started'; wait \"$task_pid\"",
        );
        request.timeout_ms = 100;
        let output = run(request).await;
        assert_eq!(output.outcome, Outcome::TimedOut);
        assert_eq!(output.stdout.text, "started");
        gone(pid(&directory.path().join("child.pid")).await).await;
    }

    #[tokio::test]
    async fn cancels_process_group() {
        for closing in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let request = launch(
                directory.path(),
                "sleep 60 & task_pid=$!; printf '%s' \"$task_pid\" > child.pid; wait \"$task_pid\"",
            );
            let stop = CancellationToken::new();
            let closed = CancellationToken::new();
            let cancelled = if closing {
                closed.clone()
            } else {
                stop.clone()
            };
            let running = tokio::spawn(async move {
                execute(request, &Environment::capture(), &stop, &closed)
                    .await
                    .unwrap()
            });
            let child = pid(&directory.path().join("child.pid")).await;
            cancelled.cancel();
            assert_eq!(running.await.unwrap().outcome, Outcome::Cancelled);
            gone(child).await;
        }
    }

    #[tokio::test]
    async fn reaps_orphaned_children() {
        let directory = tempfile::tempdir().unwrap();
        let output = run(launch(
            directory.path(),
            "sleep 60 >/dev/null 2>&1 & printf '%s' $! > child.pid; printf 'done'",
        ))
        .await;
        assert_eq!(output.outcome, Outcome::Exited(0));
        assert_eq!(output.stdout.text, "done");
        gone(pid(&directory.path().join("child.pid")).await).await;
    }

    #[tokio::test]
    async fn follows_explicit_directory_links_and_honors_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(other.path(), directory.path().join("outside")).unwrap();
        let mut request = launch(directory.path(), "pwd");
        request.cwd = "outside".into();
        let output = run(request).await;
        assert_eq!(output.outcome, Outcome::Exited(0));
        assert_eq!(
            output.stdout.text.trim(),
            other.path().canonicalize().unwrap().to_str().unwrap()
        );
        assert!(!other.path().join("marker").exists());
        let stop = CancellationToken::new();
        stop.cancel();
        let output = execute(
            launch(directory.path(), "touch marker"),
            &Environment::capture(),
            &stop,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(output.outcome, Outcome::Cancelled);
        assert!(!directory.path().join("marker").exists());
    }
}
