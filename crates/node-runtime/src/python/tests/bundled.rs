use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use zip::write::SimpleFileOptions;

struct Index {
    url: String,
    downloads: Arc<AtomicUsize>,
    stopped: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Index {
    fn start(wheel: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let downloads = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicBool::new(false));
        let count = downloads.clone();
        let stop = stopped.clone();
        let thread = std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n")
                    && request.len() < 16 * 1024
                {
                    let size = socket.read(&mut buffer).unwrap();
                    if size == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..size]);
                }
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let first = String::from_utf8_lossy(&request);
                let path = first
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                let (content_type, bytes) = if path.ends_with(".whl") {
                    count.fetch_add(1, Ordering::SeqCst);
                    ("application/octet-stream", wheel.clone())
                } else {
                    (
                        "text/html",
                        b"<a href=\"/sailry_fixture-1.0.0-py3-none-any.whl\">sailry-fixture</a>"
                            .to_vec(),
                    )
                };
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: public, max-age=31536000\r\nConnection: close\r\n\r\n",
                    bytes.len()
                );
                socket.write_all(header.as_bytes()).unwrap();
                socket.write_all(&bytes).unwrap();
            }
        });
        Self {
            url: format!("http://localhost:{}/simple", address.port()),
            downloads,
            stopped,
            thread: Some(thread),
        }
    }
}

impl Drop for Index {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let url = reqwest::Url::parse(&self.url).unwrap();
        let _ = std::net::TcpStream::connect(("127.0.0.1", url.port().unwrap()));
        self.thread.take().unwrap().join().unwrap();
    }
}

fn wheel() -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in [
        (
            "sailry_fixture/__init__.py",
            "def main():\n    print('fixture ready')\n",
        ),
        (
            "sailry_fixture-1.0.0.dist-info/METADATA",
            "Metadata-Version: 2.1\nName: sailry-fixture\nVersion: 1.0.0\n",
        ),
        (
            "sailry_fixture-1.0.0.dist-info/WHEEL",
            "Wheel-Version: 1.0\nGenerator: Sailry fixture\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
        ),
        (
            "sailry_fixture-1.0.0.dist-info/entry_points.txt",
            "[console_scripts]\nsailry-greet = sailry_fixture:main\n",
        ),
        ("sailry_fixture-1.0.0.dist-info/RECORD", ""),
    ] {
        archive
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(content.as_bytes()).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

#[tokio::test]
#[ignore = "requires SAILRY_TEST_PYTHON_RUNTIME pointing at verified bundled Python"]
async fn reuses_repairs_and_relocates_real_environments() {
    use sha2::Digest;
    let runtime = Runtime::open(&PathBuf::from(
        std::env::var("SAILRY_TEST_PYTHON_RUNTIME")
            .expect("SAILRY_TEST_PYTHON_RUNTIME is required"),
    ))
    .unwrap();
    let profile = tempfile::tempdir().unwrap();
    let manager = Manager::new(
        profile.path().canonicalize().unwrap(),
        runtime,
        Arc::new(Environment::ambient()),
    )
    .unwrap();
    let wheel = wheel();
    let hash = format!("{:x}", sha2::Sha256::digest(&wheel));
    let server = Index::start(wheel);
    manager.configure(Some(server.url.clone())).unwrap();
    let first = requirements(
        "first",
        &format!("sailry-fixture==1.0.0 --hash=sha256:{hash}\n"),
    );
    let second = requirements("second", &first.content);
    let cancel = CancellationToken::new();
    let ready = manager.ensure(Some(&first), &cancel).await.unwrap();
    assert_ne!(ready.python, manager.python());
    assert_eq!(server.downloads.load(Ordering::SeqCst), 1);
    let output = std::process::Command::new(ready.environment.join("bin/sailry-greet"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"fixture ready\n");
    let shared = manager.ensure(Some(&second), &cancel).await.unwrap();
    assert_eq!(shared.environment, ready.environment);
    assert_eq!(server.downloads.load(Ordering::SeqCst), 1);
    let package = ready
        .environment
        .join("lib/python3.12/site-packages/sailry_fixture/__init__.py");
    std::fs::write(&package, "raise RuntimeError('corrupted')\n").unwrap();
    let repaired = manager.ensure(Some(&first), &cancel).await.unwrap();
    let output = std::process::Command::new(repaired.environment.join("bin/sailry-greet"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"fixture ready\n");
    let invalid = requirements(
        "bad",
        &format!("sailry-fixture==1.0.0 --hash=sha256:{}\n", "0".repeat(64)),
    );
    assert!(manager.ensure(Some(&invalid), &cancel).await.is_err());
    assert!(
        !manager
            .root
            .join("environments")
            .join(invalid.key(&manager.runtime.identity))
            .exists()
    );
    assert_eq!(
        std::fs::read_dir(manager.root.join("staging"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        manager.subscribe().borrow().as_ref().unwrap().stage,
        Stage::Failed
    );
    let stopped = CancellationToken::new();
    stopped.cancel();
    assert!(manager.ensure(Some(&first), &stopped).await.is_err());
    assert_eq!(
        manager.ensure(None, &cancel).await.unwrap().python,
        manager.python()
    );
}
