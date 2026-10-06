use super::*;

fn binary() -> std::ffi::OsString {
    std::env::var_os("SAILRY_TEST_HOST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_sailry-host").into())
}

#[test]
fn version_does_not_open_a_profile() {
    let output = Command::new(binary()).arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!("Sailry Host {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn sharing_requires_a_running_service() {
    let directory = tempdir().unwrap();
    let profile = directory.path().join("not-started");
    let output = Command::new(binary())
        .args(["share", "--data-dir"])
        .arg(&profile)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Host is not running")
    );
    assert!(!profile.exists());
}

struct Sharing {
    child: Child,
    output: Receiver<String>,
}

impl Sharing {
    fn start(profile: &Path, origin: &str) -> Self {
        let mut child = Command::new(binary())
            .args(["share", "--data-dir"])
            .arg(profile)
            .args(["--pairing-service", origin])
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

    fn pin(&self) -> String {
        let line = self.output.recv_timeout(Duration::from_secs(15)).unwrap();
        let pin = line
            .strip_prefix("Pairing PIN: ")
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert_eq!(pin.len(), 6);
        pin.into()
    }

    fn wait(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(Instant::now() < deadline, "Sharing exit deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Sharing {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an isolated workerd instance via test/rust-e2e.mjs --host"]
async fn sharing_controls_the_existing_headless_node() {
    use sailry_client::Client;
    use sailry_link::rendezvous::{Relay, RequestId};
    use sailry_protocol::Command as NodeCommand;
    let origin = std::env::var("SAILRY_TEST_RELAY").unwrap();
    let directory = tempdir().unwrap();
    let mut host = Host::spawn(directory.path());
    host.ready();
    let controller_dir = tempdir().unwrap();
    let controller = Node::start(controller_dir.path()).await.unwrap();
    let mut sharing = Sharing::start(directory.path(), &origin);
    let pin = sharing.pin();
    let address = Relay::new(&origin)
        .unwrap()
        .pair(&controller.link(), &pin, &RequestId::default())
        .await
        .unwrap();
    assert_eq!(address.id, host.address.as_ref().unwrap().id);
    let client = Client::new(controller.link().remote(address));
    client
        .execute(client.prepare(NodeCommand::Snapshot))
        .await
        .unwrap();
    assert_eq!(
        sharing.output.recv_timeout(Duration::from_secs(5)).unwrap(),
        "Pairing complete"
    );
    sharing.wait();
    assert!(host.child.try_wait().unwrap().is_none());
    assert!(
        host.output.try_recv().is_err(),
        "The service must not log pairing PINs"
    );
    assert!(Node::start(directory.path()).await.is_err());

    let mut cancelled = Sharing::start(directory.path(), &origin);
    let pin = cancelled.pin();
    assert!(
        Command::new("kill")
            .arg("-INT")
            .arg(cancelled.child.id().to_string())
            .status()
            .unwrap()
            .success()
    );
    cancelled.wait();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        Relay::new(&origin)
            .unwrap()
            .pair(&controller.link(), &pin, &RequestId::default())
            .await
            .is_err()
    );
    assert!(host.child.try_wait().unwrap().is_none());
    host.stop();
    assert!(!directory.path().join("control.sock").exists());
    controller.shutdown().await.unwrap();
}
