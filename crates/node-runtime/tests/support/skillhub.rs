//! A signed registry fixture with real archive bytes and namespace-bound requests.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Write},
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub struct Registry {
    pub endpoint: String,
    pub state: Arc<Mutex<State>>,
    worker: tokio::task::JoinHandle<()>,
}
#[derive(Default)]
pub struct State {
    pub newer: bool,
    pub corrupt_archive: bool,
    pub unsigned: bool,
    pub requests: Vec<String>,
}

pub fn package(version: &str) -> (Vec<u8>, String, usize) {
    let files = [
        (
            "SKILL.md",
            format!(
                "---\nname: analysis\ndescription: Inspect a sample\n---\nRevision {version}\n"
            ),
        ),
        ("references/guide.md", "Fixture content".into()),
        ("LICENSE", "Fixture source license".into()),
        ("_meta.json", "{\"fixture\":true}".into()),
    ];
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut hashes = std::collections::BTreeMap::new();
    for (path, text) in files {
        zip.start_file(
            path,
            zip::write::SimpleFileOptions::default().last_modified_time(zip::DateTime::default()),
        )
        .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
        if path != "_meta.json" {
            hashes.insert(path, format!("{:x}", Sha256::digest(text.as_bytes())));
        }
    }
    let mut digest = Sha256::new();
    for (path, hash) in &hashes {
        digest.update(format!("{path}:{hash}\n"));
    }
    (
        zip.finish().unwrap().into_inner(),
        format!("{:x}", digest.finalize()),
        hashes.len(),
    )
}

pub fn signed(
    version: &str,
    archive: &[u8],
    hash: &str,
    count: usize,
) -> (serde_json::Value, serde_json::Value) {
    let key = SigningKey::from_bytes(&[27; 32]);
    let payload = serde_json::json!({"v":1,"issuer":"skillhub.cn","skill_slug":"analysis","skill_version":version,"version_id":if version == "1.0.0" {1} else {2},"publisher_id":42,"content_hash":hash,"package_md5":format!("{:x}",md5::Md5::digest(archive)),"file_count":count}).to_string();
    let signature = key.sign(payload.as_bytes());
    (
        serde_json::json!({"signed":true,"key_id":"fixture","hash_version":1,"content_hash":hash,"payload":payload,"signature":STANDARD.encode(signature.to_bytes())}),
        serde_json::json!({"keys":[{"key_id":"fixture","algorithm":"Ed25519","public_key_raw_b64":STANDARD.encode(key.verifying_key().to_bytes()),"status":"active","issuer":"skillhub.cn"}]}),
    )
}

impl Registry {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State::default()));
        let current = state.clone();
        let worker = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let current = current.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0; 1024];
                    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        let count = socket.read(&mut buffer).await.unwrap();
                        if count == 0 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..count]);
                        assert!(request.len() < 16 * 1024);
                    }
                    let request = String::from_utf8(request).unwrap();
                    let path = request
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .nth(1)
                        .unwrap();
                    let url = url::Url::parse(&format!("http://fixture{path}")).unwrap();
                    let query = url
                        .query_pairs()
                        .collect::<std::collections::BTreeMap<_, _>>();
                    let (status, headers, body) = {
                        let mut state = current.lock().unwrap();
                        state.requests.push(path.into());
                        let version = if state.newer { "1.1.0" } else { "1.0.0" };
                        let selected = query.get("version").map_or(version, |value| value.as_ref());
                        let (mut archive, hash, count) = package(selected);
                        let (_, keys) = signed(selected, &archive, &hash, count);
                        if state.corrupt_archive {
                            archive[0] ^= 1;
                        }
                        match url.path() {
                            "/api/v1/skills/analysis" => ("200 OK", String::new(), serde_json::json!({"namespace":{"handle":"example"},"skill":{"slug":"analysis"},"latestVersion":{"version":version}}).to_string().into_bytes()),
                            "/api/v1/open/platform/keys" => ("200 OK", String::new(), keys.to_string().into_bytes()),
                            path if path.ends_with("/signature") => {
                                assert_eq!(query.get("namespace").unwrap(), "example");
                                let version = path.split('/').nth(7).unwrap();
                                let (archive, hash, count) = package(version);
                                let (mut signature, _) = signed(version, &archive, &hash, count);
                                if state.unsigned { signature["signed"] = false.into(); }
                                ("200 OK", String::new(), signature.to_string().into_bytes())
                            }
                            "/api/v1/download" => {
                                assert_eq!(query.get("namespace").unwrap(), "example");
                                assert_eq!(query.get("slug").unwrap(), "analysis");
                                ("302 Found", format!("Location: /archive?version={selected}\r\n"), Vec::new())
                            }
                            "/archive" => ("200 OK", String::new(), archive),
                            _ => ("404 Not Found", String::new(), Vec::new()),
                        }
                    };
                    let header = format!(
                        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    socket.write_all(header.as_bytes()).await.unwrap();
                    socket.write_all(&body).await.unwrap();
                });
            }
        });
        Self {
            endpoint,
            state,
            worker,
        }
    }
}
impl Drop for Registry {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
