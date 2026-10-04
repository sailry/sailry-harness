#![cfg(target_os = "macos")]

use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command as NodeCommand, Output};
use std::{
    ffi::OsStr,
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

struct Desktop {
    child: Child,
    output: Receiver<String>,
}

impl Desktop {
    fn spawn(profile: &Path) -> Self {
        Self::with_args(&[OsStr::new("--data-dir"), profile.as_os_str()])
    }

    fn with_args(args: &[&OsStr]) -> Self {
        let binary = std::env::var_os("SAILRY_TEST_DESKTOP_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_sailry-desktop").into());
        let mut child = Command::new(binary)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sender.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        Self { child, output }
    }

    fn ready(&mut self) {
        assert_eq!(
            self.output.recv_timeout(Duration::from_secs(15)).unwrap(),
            "Desktop Node ready"
        );
        assert_eq!(
            self.output.recv_timeout(Duration::from_secs(15)).unwrap(),
            "Desktop window ready"
        );
        assert!(self.child.try_wait().unwrap().is_none());
    }

    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "Desktop exit deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn stop(&mut self, signal: &str) {
        if signal == "native" {
            let application =
                objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(
                    self.child.id() as i32,
                )
                .expect("native Desktop application");
            assert!(application.terminate(), "native quit request accepted");
        } else {
            assert!(
                Command::new("kill")
                    .arg(signal)
                    .arg(self.child.id().to_string())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        assert!(self.wait().success());
        assert_eq!(
            self.output.recv_timeout(Duration::from_secs(1)).unwrap(),
            "Desktop Node stopped"
        );
        let mut errors = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut errors)
            .unwrap();
        assert!(!errors.contains("shutdown failed"), "{errors}");
        assert!(!errors.contains("app_will_quit"), "{errors}");
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
#[ignore = "opens real macOS Desktop processes; run explicitly with --ignored"]
fn preserves_profile() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("desktop");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let node = runtime.block_on(Node::start(&profile)).unwrap();
    let identity = node.id();
    let client = Client::new(node.local());
    let request = client.prepare(NodeCommand::RegisterProject {
        name: "Persistent project".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let project = runtime.block_on(client.execute(request.clone())).unwrap();
    runtime.block_on(node.shutdown()).unwrap();

    for signal in ["-TERM", "-INT", "native"] {
        let mut desktop = Desktop::spawn(&profile);
        desktop.ready();
        let mut competing = Desktop::spawn(&profile);
        assert!(!competing.wait().success());
        assert!(runtime.block_on(Node::start(&profile)).is_err());
        desktop.stop(signal);
        let restored = runtime.block_on(Node::start(&profile)).unwrap();
        assert_eq!(restored.id(), identity);
        let client = Client::new(restored.local());
        assert_eq!(
            runtime.block_on(client.execute(request.clone())).unwrap(),
            project
        );
        let Output::Snapshot(snapshot) = runtime
            .block_on(client.execute(client.prepare(NodeCommand::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected");
        };
        assert_eq!(snapshot.projects.len(), 1);
        assert_eq!(snapshot.worktrees.len(), 1);
        runtime.block_on(restored.shutdown()).unwrap();
    }
}

#[test]
fn validates_options() {
    let mut help = Desktop::with_args(&[OsStr::new("--help")]);
    assert!(help.wait().success());
    assert!(
        help.output
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .starts_with("Usage: sailry-desktop")
    );
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("unused");
    let mut invalid = Desktop::with_args(&[
        OsStr::new("--preview"),
        OsStr::new("--data-dir"),
        profile.as_os_str(),
    ]);
    assert!(!invalid.wait().success());
    assert!(!profile.exists());
}

#[test]
#[ignore = "opens a real macOS preview; run explicitly with --ignored"]
fn preview_without_node() {
    let mut preview = Desktop::with_args(&[OsStr::new("--preview")]);
    assert_eq!(
        preview
            .output
            .recv_timeout(Duration::from_secs(15))
            .unwrap(),
        "Preview window ready"
    );
    let application = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(
        preview.child.id() as i32,
    )
    .expect("native preview application");
    assert!(application.terminate());
    assert!(preview.wait().success());
    assert!(
        preview
            .output
            .recv_timeout(Duration::from_millis(100))
            .is_err()
    );
}
