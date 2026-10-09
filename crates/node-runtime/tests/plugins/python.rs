use super::*;
use std::{
    io::{Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

struct Index {
    address: std::net::SocketAddr,
    downloads: Arc<AtomicUsize>,
    stopped: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Index {
    fn start(wheel: Vec<u8>) -> Self {
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let downloads = Arc::new(AtomicUsize::new(0));
        let stop = stopped.clone();
        let count = downloads.clone();
        let worker = std::thread::spawn(move || {
            for stream in server.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let mut socket = stream.unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") && request.len() < 16 * 1024 {
                    let mut byte = [0];
                    if socket.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    request.extend(byte);
                }
                let is_wheel = String::from_utf8_lossy(&request)
                    .lines()
                    .next()
                    .is_some_and(|line| line.contains(".whl"));
                let bytes = if is_wheel {
                    count.fetch_add(1, Ordering::SeqCst);
                    wheel.clone()
                } else {
                    b"<a href='/sailry_fixture-1.0.0-py3-none-any.whl'>fixture</a>".to_vec()
                };
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    if is_wheel {
                        "application/octet-stream"
                    } else {
                        "text/html"
                    },
                    bytes.len()
                );
                socket.write_all(header.as_bytes()).unwrap();
                socket.write_all(&bytes).unwrap();
            }
        });
        Self {
            address,
            stopped,
            downloads,
            worker: Some(worker),
        }
    }
}

impl Drop for Index {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(self.address);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn wheel() -> Vec<u8> {
    use zip::write::SimpleFileOptions;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, content) in [
        ("sailry_fixture/__init__.py", "value = 'managed fixture'\n"),
        (
            "sailry_fixture-1.0.0.dist-info/METADATA",
            "Metadata-Version: 2.1\nName: sailry-fixture\nVersion: 1.0.0\n",
        ),
        (
            "sailry_fixture-1.0.0.dist-info/WHEEL",
            "Wheel-Version: 1.0\nGenerator: Sailry\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
        ),
        ("sailry_fixture-1.0.0.dist-info/RECORD", ""),
    ] {
        zip.start_file(path, SimpleFileOptions::default()).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[tokio::test]
#[ignore = "requires SAILRY_TEST_PYTHON_RUNTIME pointing at verified bundled Python"]
async fn prepares_before_install_and_enable() {
    use sha2::Digest;
    let runtime = std::path::PathBuf::from(
        std::env::var_os("SAILRY_TEST_PYTHON_RUNTIME")
            .expect("SAILRY_TEST_PYTHON_RUNTIME is required"),
    );
    let bytes = wheel();
    let requirements = format!(
        "sailry-fixture==1.0.0 --hash=sha256:{:x}\n",
        sha2::Sha256::digest(&bytes)
    );
    let index = Index::start(bytes);
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        node.set_python_runtime_fixture(&runtime).unwrap();
        assert!(!node.profile().join("python/environments").exists());
        execute(
            &client,
            Command::SavePythonSettings(sailry_protocol::python::Settings {
                revision: 0,
                index: Some(format!("http://localhost:{}/simple", index.address.port())),
            }),
        )
        .await;
        let lock = directory.path().join("source/package/requirements.txt");
        fs::write(&lock, &requirements).unwrap();
        let request = client.prepare(install(worktree, 0));
        let installed = info(client.execute(request.clone()).await.unwrap());
        let environments: Vec<_> = fs::read_dir(node.profile().join("python/environments"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(environments.len(), 1);
        assert!(environments[0].join(".sailry-ready.json").is_file());
        let downloads = index.downloads.load(Ordering::SeqCst);
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::Plugin(installed.clone())
        );
        assert_eq!(index.downloads.load(Ordering::SeqCst), downloads);
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: installed.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        let captured_lock = node
            .profile()
            .join("plugins/packages")
            .join(&disabled.summary.digest)
            .join("requirements.txt");
        fs::write(&captured_lock, "changed immutable package").unwrap();
        let failed = client.prepare(Command::SetPluginEnabled {
            name: "example".into(),
            expected_revision: disabled.summary.revision,
            enabled: true,
        });
        let failure = client.execute(failed.clone()).await.unwrap_err();
        assert_eq!(failure.code, ErrorCode::Unavailable);
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "example".into()
                    }
                )
                .await
            ),
            disabled
        );
        fs::write(&captured_lock, &requirements).unwrap();
        let enabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: disabled.summary.revision,
                    enabled: true,
                },
            )
            .await,
        );
        assert!(enabled.summary.enabled);
        assert_eq!(index.downloads.load(Ordering::SeqCst), downloads);
        assert_eq!(
            client.outcome(&failed).await.unwrap(),
            RequestOutcome::Completed(Box::new(Err(failure)))
        );
        let stored = fs::read(&lock).unwrap();
        fs::write(
            &lock,
            format!("sailry-fixture==1.0.0 --hash=sha256:{}\n", "0".repeat(64)),
        )
        .unwrap();
        assert!(
            client
                .execute(client.prepare(install(worktree, enabled.summary.revision)))
                .await
                .is_err()
        );
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "example".into()
                    }
                )
                .await
            ),
            enabled
        );
        fs::write(&lock, stored).unwrap();
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
