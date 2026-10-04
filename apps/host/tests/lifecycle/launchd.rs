use super::*;
use std::os::unix::fs::PermissionsExt;

struct Job {
    target: String,
    loaded: bool,
}

impl Job {
    fn stop(&mut self) {
        assert!(
            Command::new("launchctl")
                .args(["bootout", &self.target])
                .status()
                .unwrap()
                .success()
        );
        self.loaded = false;
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if self.loaded {
            let _ = Command::new("launchctl")
                .args(["bootout", &self.target])
                .output();
        }
    }
}

fn until(state: &str, mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(25);
    while !predicate() {
        assert!(Instant::now() < deadline, "service {state} deadline");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "registers an isolated macOS user service; run explicitly with --ignored"]
fn restarts_and_unloads() {
    let directory = tempdir().unwrap();
    let profile = directory.path().join("profile with spaces");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let node = runtime.block_on(Node::start(&profile)).unwrap();
    let identity = node.id();
    runtime.block_on(node.shutdown()).unwrap();
    let binary = std::env::var_os("SAILRY_TEST_HOST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_sailry-host").into());
    let installed = directory.path().join("sailry-host");
    std::fs::copy(binary, &installed).unwrap();
    let output = Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/host-launchd.sh"
        ))
        .arg(installed)
        .arg(&profile)
        .args(["--bind", "127.0.0.1:0"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    let plist = output
        .lines()
        .find_map(|line| line.strip_prefix("Service definition: "))
        .unwrap();
    let label = format!(
        "ai.sailry.host.acceptance.{}",
        directory.path().file_name().unwrap().to_str().unwrap()
    );
    assert!(
        Command::new("plutil")
            .args(["-replace", "Label", "-string", &label, plist])
            .status()
            .unwrap()
            .success()
    );
    let user = Command::new("id").arg("-u").output().unwrap();
    let domain = format!("gui/{}", String::from_utf8(user.stdout).unwrap().trim());
    assert!(
        Command::new("launchctl")
            .args(["bootstrap", &domain, plist])
            .status()
            .unwrap()
            .success()
    );
    let mut job = Job {
        target: format!("{domain}/{label}"),
        loaded: true,
    };
    let stdout = profile.join("host.stdout.log");
    let ready = || {
        std::fs::read_to_string(&stdout)
            .unwrap_or_default()
            .matches("Node ready:")
            .count()
    };
    until("startup", || ready() == 1);
    assert!(runtime.block_on(Node::start(&profile)).is_err());
    assert_eq!(
        std::fs::metadata(&profile).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(
        Command::new("launchctl")
            .args(["kill", "SIGKILL", &job.target])
            .status()
            .unwrap()
            .success()
    );
    until("restart", || ready() == 2);
    job.stop();
    until("shutdown", || {
        std::fs::read_to_string(&stdout)
            .unwrap()
            .contains("Node stopped")
    });
    let restored = runtime.block_on(Node::start(&profile)).unwrap();
    assert_eq!(restored.id(), identity);
    runtime.block_on(restored.shutdown()).unwrap();
}
