#![cfg(unix)]
use super::*;
use crate::process::{
    Access,
    environment::{EnvironmentFixture, Source},
};
use std::{collections::BTreeMap, path::Path};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

struct Running {
    task: Option<JoinHandle<Result<Completion, Fault>>>,
    input: mpsc::Sender<Vec<u8>>,
    output: watch::Receiver<(Capture, Capture)>,
    stop: CancellationToken,
    closed: CancellationToken,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Running {
    async fn start(root: &Path, home: &Path, command: &str, access: Access) -> Self {
        std::fs::create_dir_all(home).unwrap();
        let environment = Environment::capture(Source::Fixture(EnvironmentFixture {
            home: home.into(),
            shell: "/bin/false".into(),
            environment: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
            timeout: Duration::from_secs(1),
        }))
        .await;
        let launch = Launch {
            root: root.canonicalize().unwrap(),
            profile: None,
            command: command.into(),
            access,
            pty: true,
            cwd: String::new(),
            timeout_ms: 1,
            inputs: None,
            scratch: None,
        };
        let stop = CancellationToken::new();
        let closed = CancellationToken::new();
        let (progress, output) = watch::channel((Capture::default(), Capture::default()));
        let (input, received) = mpsc::channel(32);
        let cancelled = stop.clone();
        let closing = closed.clone();
        let task = tokio::spawn(async move {
            execute(
                launch,
                &environment,
                &cancelled,
                &closing,
                &progress,
                received,
            )
            .await
        });
        Self {
            task: Some(task),
            input,
            output,
            stop,
            closed,
        }
    }

    async fn ready(&mut self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if self.output.borrow().0.text.contains("ready") {
                    return;
                }
                self.output
                    .changed()
                    .await
                    .expect("PTY output ended before readiness");
            }
        })
        .await
        .expect("PTY readiness deadline");
    }

    async fn finish(mut self) -> Completion {
        tokio::time::timeout(Duration::from_secs(6), self.task.take().unwrap())
            .await
            .expect("PTY completion deadline")
            .unwrap()
            .unwrap()
    }
}

async fn gone(pid: i32) {
    tokio::time::timeout(Duration::from_secs(3), async {
        // SAFETY: signal 0 only probes the exact PID written by this fixture.
        while unsafe { libc::kill(pid, 0) } == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("PTY descendant did not exit");
}

fn child(root: &Path) -> i32 {
    std::fs::read_to_string(root.join("child.pid"))
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn accepts_unicode_input_and_combines_output() {
    let directory = tempfile::tempdir().unwrap();
    let mut running = Running::start(directory.path(), &directory.path().join("home"),
        "printf 'ready\\n'; IFS= read -r answer; printf 'accepted:%s\\n' \"$answer\"; printf 'stderr tail\\n' >&2; exit 7", Access::Unrestricted).await;
    running.ready().await;
    running
        .input
        .send("中文 🙂\n".as_bytes().to_vec())
        .await
        .unwrap();
    let result = running.finish().await;
    assert_eq!(result.outcome, Outcome::Exited(7));
    assert!(result.stdout.text.contains("accepted:中文 🙂"));
    assert!(result.stdout.text.ends_with("stderr tail\r\n"));
    assert_eq!(result.stderr, Capture::default());
    assert!(!result.stdout.invalid_utf8);
}

#[tokio::test]
async fn drains_the_bounded_unicode_tail() {
    let directory = tempfile::tempdir().unwrap();
    let running = Running::start(directory.path(), &directory.path().join("home"),
        "printf 'initial marker'; dd if=/dev/zero bs=1024 count=96 2>/dev/null; printf '中文 🙂 final tail\\n'; exit 7", Access::Unrestricted).await;
    let result = running.finish().await;
    assert_eq!(result.outcome, Outcome::Exited(7));
    assert!(result.stdout.truncated);
    assert!(!result.stdout.invalid_utf8);
    assert!(result.stdout.text.len() <= 64 * 1024);
    assert!(!result.stdout.text.contains("initial marker"));
    assert!(result.stdout.text.ends_with("中文 🙂 final tail\r\n"));
}

#[tokio::test]
async fn cancellation_stops_idle_process_groups() {
    for closing in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut running = Running::start(directory.path(), &directory.path().join("home"),
            "sleep 60 & child_pid=$!; printf '%s' \"$child_pid\" > child.pid; printf 'ready'; wait \"$child_pid\"", Access::Unrestricted).await;
        running.ready().await;
        let pid = child(directory.path());
        if closing {
            running.closed.cancel();
        } else {
            running.stop.cancel();
        }
        assert_eq!(running.finish().await.outcome, Outcome::Cancelled);
        gone(pid).await;
    }
}

#[tokio::test]
async fn backpressure_preserves_cancellation() {
    let directory = tempfile::tempdir().unwrap();
    let mut running = Running::start(directory.path(), &directory.path().join("home"),
        "stty raw -echo; sleep 60 & child_pid=$!; printf '%s' \"$child_pid\" > child.pid; printf 'ready'; wait \"$child_pid\"", Access::Unrestricted).await;
    running.ready().await;
    let pid = child(directory.path());
    let input = running.input.clone();
    let sending = tokio::spawn(async move {
        for _ in 0..80 {
            if input.send(vec![b'x'; 64 * 1024]).await.is_err() {
                break;
            }
        }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !running.task.as_ref().unwrap().is_finished(),
        "input backpressure terminated the command"
    );
    running.stop.cancel();
    assert_eq!(running.finish().await.outcome, Outcome::Cancelled);
    tokio::time::timeout(Duration::from_secs(1), sending)
        .await
        .unwrap()
        .unwrap();
    gone(pid).await;
}

#[tokio::test]
async fn root_exit_stops_remaining_children() {
    let directory = tempfile::tempdir().unwrap();
    let running = Running::start(
        directory.path(),
        &directory.path().join("home"),
        "sleep 60 >/dev/null 2>&1 & printf '%s' \"$!\" > child.pid; printf 'done'",
        Access::Unrestricted,
    )
    .await;
    assert_eq!(running.finish().await.outcome, Outcome::Exited(0));
    gone(child(directory.path())).await;
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[tokio::test]
async fn keeps_workspace_confinement_with_a_pty() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let home = directory.path().join("home");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::write(home.join(".ssh/fixture-key"), "isolated fixture credential").unwrap();
    let outside = directory.path().join("outside.txt");
    let quote = |path: &Path| format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let network = if Path::new("/usr/bin/curl").is_file() {
        format!(
            "if /usr/bin/curl --connect-timeout 1 --max-time 2 -s http://127.0.0.1:{port}/ >/dev/null 2>&1; then printf 'network escaped\\n'; else printf 'network denied\\n'; fi;"
        )
    } else {
        String::new()
    };
    let command = format!(
        "printf allowed > allowed.txt; if printf escaped > {} 2>/dev/null; then printf 'write escaped\\n'; else printf 'write denied\\n'; fi; if cat {} >/dev/null 2>&1; then printf 'credential readable\\n'; else printf 'credential denied\\n'; fi; {network} printf 'private-home:%s\\n' \"$HOME\"",
        quote(&outside),
        quote(&home.join(".ssh/fixture-key"))
    );
    let running = Running::start(&root, &home, &command, Access::Workspace).await;
    let result = running.finish().await;
    assert_eq!(result.outcome, Outcome::Exited(0));
    assert_eq!(
        std::fs::read_to_string(root.join("allowed.txt")).unwrap(),
        "allowed"
    );
    assert!(!outside.exists());
    assert!(result.stdout.text.contains("write denied"));
    assert!(result.stdout.text.contains("credential denied"));
    assert!(!result.stdout.text.contains(home.to_str().unwrap()));
    if !network.is_empty() {
        assert!(result.stdout.text.contains("network denied"));
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }
}
