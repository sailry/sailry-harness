#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use sailry_node_runtime::Node;
#[path = "lifecycle/control.rs"]
mod control;
#[cfg(target_os = "macos")]
#[path = "lifecycle/launchd.rs"]
mod launchd;
#[path = "lifecycle/workload.rs"]
mod workload;

fn tempdir() -> std::io::Result<tempfile::TempDir> {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    Ok(directory)
}

struct Host {
    child: Child,
    output: Receiver<String>,
    address: Option<sailry_node_runtime::EndpointAddr>,
}

impl Host {
    fn spawn(profile: &Path) -> Self {
        Self::with_args(profile, &[])
    }

    fn with_args(profile: &Path, args: &[&str]) -> Self {
        let binary = std::env::var_os("SAILRY_TEST_HOST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_sailry-host").into());
        let mut child = Command::new(binary)
            .arg("--data-dir")
            .arg(profile)
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
        Self {
            child,
            output,
            address: None,
        }
    }

    fn ready(&mut self) {
        let line = self
            .output
            .recv_timeout(Duration::from_secs(10))
            .expect("Host readiness deadline");
        assert!(line.starts_with("Node ready:"), "unexpected output: {line}");
        let line = self.output.recv_timeout(Duration::from_secs(2)).unwrap();
        self.address =
            Some(serde_json::from_str(line.strip_prefix("Link address: ").unwrap()).unwrap());
        assert!(self.child.try_wait().unwrap().is_none());
    }

    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "Host exit deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn stop(&mut self) {
        assert!(
            Command::new("kill")
                .arg("-TERM")
                .arg(self.child.id().to_string())
                .status()
                .unwrap()
                .success()
        );
        assert!(self.wait().success());
        let line = self.output.recv_timeout(Duration::from_secs(1)).unwrap();
        let line = if line == "Pairing closed" {
            self.output.recv_timeout(Duration::from_secs(1)).unwrap()
        } else {
            line
        };
        assert_eq!(line, "Node stopped");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an isolated workerd instance via test/rust-e2e.mjs --host"]
async fn short_code_pairs_a_headless_process() {
    use sailry_client::Client;
    use sailry_link::rendezvous::{Relay, RequestId};
    use sailry_protocol::Command as NodeCommand;
    let origin = std::env::var("SAILRY_TEST_RELAY").unwrap();
    let directory = tempdir().unwrap();
    let mut host = Host::with_args(directory.path(), &["--pairing-service", &origin]);
    host.ready();
    let line = host.output.recv_timeout(Duration::from_secs(10)).unwrap();
    let code = line
        .strip_prefix("Pairing PIN: ")
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    assert_eq!(code.len(), 6);
    let controller_dir = tempdir().unwrap();
    let controller = Node::start(controller_dir.path()).await.unwrap();
    let address = Relay::new(&origin)
        .unwrap()
        .pair(&controller.link(), code, &RequestId::default())
        .await
        .unwrap();
    let client = Client::new(controller.link().remote(address));
    client
        .execute(client.prepare(NodeCommand::Snapshot))
        .await
        .unwrap();
    assert_eq!(
        host.output.recv_timeout(Duration::from_secs(5)).unwrap(),
        "Pairing complete; sharing stopped"
    );
    host.stop();
    let restored = Node::start(directory.path()).await.unwrap();
    assert_eq!(restored.link().peers().await.unwrap().len(), 1);
    restored.shutdown().await.unwrap();
    let mut sharing = Host::with_args(directory.path(), &["--pairing-service", &origin]);
    sharing.ready();
    let line = sharing
        .output
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    let code = line
        .strip_prefix("Pairing PIN: ")
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    sharing.stop();
    assert!(
        Relay::new(&origin)
            .unwrap()
            .pair(&controller.link(), code, &RequestId::default())
            .await
            .is_err()
    );
    controller.shutdown().await.unwrap();
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn runs_headless_and_shuts_down() {
    let directory = tempdir().unwrap();
    let mut host = Host::spawn(directory.path());
    host.ready();
    let mut competing = Host::spawn(directory.path());
    assert!(!competing.wait().success());
    let mut error = String::new();
    std::io::Read::read_to_string(&mut competing.child.stderr.take().unwrap(), &mut error).unwrap();
    assert!(error.contains("a Node already owns profile"), "{error}");
    assert!(host.child.try_wait().unwrap().is_none());
    host.stop();
    assert!(directory.path().join("node.lock").exists());
    let mut restarted = Host::spawn(directory.path());
    restarted.ready();
    restarted.stop();
}

#[test]
fn separate_profiles_run_concurrently() {
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    let mut first = Host::spawn(first.path());
    let mut second = Host::spawn(second.path());
    first.ready();
    second.ready();
    first.stop();
    assert!(second.child.try_wait().unwrap().is_none());
    second.stop();
}

#[tokio::test]
async fn enforces_single_profile_owner() {
    let directory = tempdir().unwrap();
    let embedded = Node::start(directory.path()).await.unwrap();
    let mut host = Host::spawn(directory.path());
    assert!(!host.wait().success());
    embedded.observe().probe().await.unwrap();
    embedded.shutdown().await.unwrap();
    let mut host = Host::spawn(directory.path());
    host.ready();
    assert!(Node::start(directory.path()).await.is_err());
    host.stop();
}

#[test]
fn crash_releases_lock() {
    let directory = tempdir().unwrap();
    let mut host = Host::spawn(directory.path());
    host.ready();
    host.child.kill().unwrap();
    assert!(!host.wait().success());
    let mut restarted = Host::spawn(directory.path());
    restarted.ready();
    restarted.stop();
}

#[tokio::test]
async fn client_controls_host_process() {
    use sailry_client::Client;
    use sailry_protocol::{Command as NodeCommand, Output};
    let directory = tempdir().unwrap();
    let controller = Node::start(directory.path().join("controller"))
        .await
        .unwrap();
    let profile = directory.path().join("host");
    let prepared = Node::start(&profile).await.unwrap();
    prepared
        .link()
        .set_trust(controller.id(), true)
        .await
        .unwrap();
    controller
        .link()
        .set_trust(prepared.id(), true)
        .await
        .unwrap();
    prepared.shutdown().await.unwrap();
    let mut host = Host::spawn(&profile);
    host.ready();
    let client = Client::new(controller.link().remote(host.address.clone().unwrap()));
    let request = client.prepare(NodeCommand::RegisterProject {
        name: "Headless project".into(),
        path: directory.path().to_str().unwrap().into(),
    });
    let original = client.execute(request.clone()).await.unwrap();
    assert!(matches!(original, Output::Project(_)));
    host.stop();
    let mut host = Host::spawn(&profile);
    host.ready();
    let client = Client::new(controller.link().remote(host.address.clone().unwrap()));
    assert_eq!(client.execute(request).await.unwrap(), original);
    host.stop();
    controller.shutdown().await.unwrap();
}
