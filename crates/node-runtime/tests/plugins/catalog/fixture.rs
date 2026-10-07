use std::{
    io::{Cursor, Write},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub(super) const FIRST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const SECOND: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

pub(super) struct Catalog {
    pub endpoint: String,
    pub updated: Arc<AtomicBool>,
    pub status: Arc<AtomicU16>,
    pub invalid: Arc<AtomicBool>,
    pub requests: Arc<Mutex<Vec<String>>>,
    pub version: Arc<Mutex<Option<String>>>,
    worker: tokio::task::JoinHandle<()>,
}

impl Catalog {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let updated = Arc::new(AtomicBool::new(false));
        let current = updated.clone();
        let status = Arc::new(AtomicU16::new(200));
        let response_status = status.clone();
        let invalid = Arc::new(AtomicBool::new(false));
        let malformed = invalid.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let version = Arc::new(Mutex::new(None::<String>));
        let selected_version = version.clone();
        let first = archive("1.0.0", "0.1.0");
        let second = archive("2.0.0", "0.2.0");
        let worker = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                while !bytes.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut buffer = [0; 1024];
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert!(count > 0 && bytes.len() < 16 * 1024);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(!request.to_lowercase().contains("authorization:"));
                let path = request
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                captured.lock().unwrap().push(path.to_owned());
                let (code, content_type, body) = match path {
                    "/catalog.json" => (
                        response_status.load(Ordering::SeqCst),
                        "application/json",
                        if malformed.load(Ordering::SeqCst) {
                            b"{\"version\":2,\"packages\":[]}".to_vec()
                        } else {
                            index(
                                current.load(Ordering::SeqCst),
                                selected_version.lock().unwrap().as_deref(),
                            )
                        },
                    ),
                    "/repos/sailry/sailry-plugins/commits/main" => (
                        200,
                        "application/vnd.github.sha",
                        if current.load(Ordering::SeqCst) {
                            SECOND
                        } else {
                            FIRST
                        }
                        .as_bytes()
                        .to_vec(),
                    ),
                    path if path == format!("/sailry/sailry-plugins/zip/{FIRST}") => {
                        (200, "application/zip", first.clone())
                    }
                    path if path == format!("/sailry/sailry-plugins/zip/{SECOND}") => {
                        (200, "application/zip", second.clone())
                    }
                    _ => panic!("unexpected catalog fixture path: {path}"),
                };
                socket.write_all(format!(
                    "HTTP/1.1 {code} Fixture\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()
                ).as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
                socket.shutdown().await.unwrap();
            }
        });
        Self {
            endpoint,
            updated,
            status,
            invalid,
            requests,
            version,
            worker,
        }
    }
}

impl Drop for Catalog {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

fn manifest(version: &str) -> serde_json::Value {
    serde_json::json!({
        "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name": "official-new", "version": version, "description": "New official package",
    })
}

fn index(updated: bool, version: Option<&str>) -> Vec<u8> {
    let mut index: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../plugins/catalog.json")).unwrap();
    index["packages"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": "official-new", "manifest": manifest(if updated { "2.0.0" } else { "1.0.0" }),
        }));
    let package = index["packages"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|package| package["id"] == "reminders")
        .unwrap();
    package["manifest"]["version"] = version
        .unwrap_or(if updated { "0.2.0" } else { "0.1.0" })
        .into();
    serde_json::to_vec(&index).unwrap()
}

fn archive(version: &str, reminders: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
    zip.start_file("repo-source/LICENSE", options).unwrap();
    zip.write_all(b"Fixture root license").unwrap();
    zip.start_file("repo-source/official-new/plugin.json", options)
        .unwrap();
    zip.write_all(&serde_json::to_vec(&manifest(version)).unwrap())
        .unwrap();
    zip.start_file("repo-source/official-new/LICENSE", options)
        .unwrap();
    zip.write_all(b"Fixture package license").unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/reminders");
    files(&root, &root, &mut zip, reminders);
    zip.start_file("repo-source/reminders/fixture-revision.txt", options)
        .unwrap();
    zip.write_all(version.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

fn files(root: &Path, directory: &Path, zip: &mut zip::ZipWriter<Cursor<Vec<u8>>>, version: &str) {
    let mut entries = std::fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            files(root, &path, zip, version);
        } else {
            assert!(entry.file_type().unwrap().is_file());
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            zip.start_file(
                format!("repo-source/reminders/{relative}"),
                zip::write::SimpleFileOptions::default().unix_permissions(0o644),
            )
            .unwrap();
            let mut bytes = std::fs::read(path).unwrap();
            if relative == "plugin.json" {
                let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                manifest["version"] = version.into();
                bytes = serde_json::to_vec(&manifest).unwrap();
            }
            zip.write_all(&bytes).unwrap();
        }
    }
}
