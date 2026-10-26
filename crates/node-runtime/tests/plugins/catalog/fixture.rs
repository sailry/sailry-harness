mod external;
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

pub(super) fn bundled_version() -> String {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../plugins/reminders/plugin.json")).unwrap();
    manifest["version"].as_str().unwrap().to_owned()
}

pub(super) struct Catalog {
    pub endpoint: String,
    pub updated: Arc<AtomicBool>,
    pub status: Arc<AtomicU16>,
    pub invalid: Arc<AtomicBool>,
    pub legacy: Arc<AtomicBool>,
    pub obsolete: Arc<AtomicBool>,
    pub requests: Arc<Mutex<Vec<String>>>,
    pub version: Arc<Mutex<Option<String>>>,
    pub documents: Arc<Mutex<std::collections::BTreeMap<String, Vec<u8>>>>,
    worker: tokio::task::JoinHandle<()>,
}

impl Catalog {
    pub fn index_paths(&self) -> Vec<String> {
        let commit = if self.updated.load(Ordering::SeqCst) {
            SECOND
        } else {
            FIRST
        };
        vec![
            "/repos/sailry/sailry-plugins/commits/main".into(),
            format!("/{commit}/catalog.json"),
        ]
    }

    pub async fn start() -> Self {
        Self::with_reminders("0.1.0").await
    }

    pub async fn with_reminders(reminders: &str) -> Self {
        Self::with_configuration(reminders, false).await
    }

    pub async fn with_mcp_reminders(reminders: &str) -> Self {
        Self::with_configuration(reminders, true).await
    }

    pub async fn with_content_updates(reminders: &str) -> Self {
        let version = bundled_version();
        let catalog = Self::with_versions(reminders, &version, false).await;
        *catalog.version.lock().unwrap() = Some(version);
        catalog
    }

    async fn with_configuration(reminders: &str, mcp: bool) -> Self {
        Self::with_versions(reminders, "0.2.0", mcp).await
    }

    pub(super) async fn with_versions(reminders: &str, updated_version: &str, mcp: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let updated = Arc::new(AtomicBool::new(false));
        let current = updated.clone();
        let status = Arc::new(AtomicU16::new(200));
        let response_status = status.clone();
        let invalid = Arc::new(AtomicBool::new(false));
        let malformed = invalid.clone();
        let legacy = Arc::new(AtomicBool::new(false));
        let old_format = legacy.clone();
        let obsolete = Arc::new(AtomicBool::new(false));
        let incompatible = obsolete.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let version = Arc::new(Mutex::new(None::<String>));
        let selected_version = version.clone();
        let documents = Arc::new(Mutex::new(
            std::collections::BTreeMap::<String, Vec<u8>>::new(),
        ));
        let responses = documents.clone();
        let first = archive("1.0.0", reminders, mcp);
        let second = archive("2.0.0", updated_version, mcp);
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
                let overridden = responses.lock().unwrap().get(path).cloned();
                let (code, content_type, body) = if let Some(body) = overridden {
                    (
                        200,
                        if path.contains("/zip/") {
                            "application/zip"
                        } else {
                            "application/json"
                        },
                        body,
                    )
                } else if let Some(response) =
                    external::response(path, response_status.load(Ordering::SeqCst))
                {
                    response
                } else {
                    match path {
                        path if path == format!("/{FIRST}/catalog.json")
                            || path == format!("/{SECOND}/catalog.json") =>
                        {
                            (
                                response_status.load(Ordering::SeqCst),
                                "application/json",
                                if malformed.load(Ordering::SeqCst) {
                                    b"{\"version\":2,\"packages\":[]}".to_vec()
                                } else {
                                    index(
                                        path == format!("/{SECOND}/catalog.json"),
                                        selected_version.lock().unwrap().as_deref(),
                                        incompatible.load(Ordering::SeqCst),
                                        old_format.load(Ordering::SeqCst),
                                    )
                                },
                            )
                        }
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
                    }
                };
                // The client rejects error status at the headers without reading its body.
                // Do not stream a complete success catalog into that closed connection.
                let body = if (200..300).contains(&code) {
                    body
                } else {
                    Vec::new()
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
            legacy,
            obsolete,
            requests,
            version,
            documents,
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

fn index(updated: bool, version: Option<&str>, obsolete: bool, legacy: bool) -> Vec<u8> {
    let mut index: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../plugins/catalog.json")).unwrap();
    // Existing fixtures describe one latest-only snapshot; history fixtures opt in explicitly.
    for package in index["packages"].as_array_mut().unwrap() {
        package.as_object_mut().unwrap().remove("versions");
    }
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
    if obsolete {
        let package = index["packages"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|package| package["id"] == "files")
            .unwrap();
        package["manifest"]["extensions"]["dev.sailry.platform"]["tools"][0]["operation"] =
            "office.export".into();
    }
    if legacy {
        for package in index["packages"].as_array_mut().unwrap() {
            if package["id"] != "official-new" {
                package["manifest"]["version"] = "0.1.0".into();
            }
            if let Some(extension) = package
                .get_mut("manifest")
                .and_then(|manifest| manifest.get_mut("extensions"))
                .and_then(|extensions| extensions.get_mut("dev.sailry.platform"))
                .and_then(serde_json::Value::as_object_mut)
            {
                extension.remove("category");
                extension.remove("tint");
            }
        }
        index.as_object_mut().unwrap().remove("skills");
    } else {
        let skills = index
            .as_object_mut()
            .unwrap()
            .entry("skills")
            .or_insert_with(|| serde_json::json!([]));
        skills.as_array_mut().unwrap().push(serde_json::json!({
            "package":"official-new", "path":"skills/fixture-skill/SKILL.md",
            "frontmatter": SKILL,
        }));
    }
    serde_json::to_vec(&index).unwrap()
}

const SKILL: &str = "---\nname: fixture-skill\ndescription: Searchable neutral skill\nmetadata:\n  display-name: Fixture skill\n  display-name.zh-CN: 测试技能\n---\n";

fn archive(version: &str, reminders: &str, mcp: bool) -> Vec<u8> {
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
    zip.start_file(
        "repo-source/official-new/skills/fixture-skill/SKILL.md",
        options,
    )
    .unwrap();
    zip.write_all(format!("{SKILL}Neutral instructions\n").as_bytes())
        .unwrap();
    let index: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../plugins/catalog.json")).unwrap();
    for package in index["packages"].as_array().unwrap() {
        let name = package["id"].as_str().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins")
            .join(name);
        files(
            &root,
            &root,
            &mut zip,
            (name == "reminders").then_some(reminders),
        );
    }
    if mcp {
        zip.start_file("repo-source/reminders/mcp.json", options)
            .unwrap();
        let configuration = serde_json::to_vec(&serde_json::json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
            "mcpServers":{"service":{
                "type":"streamable-http", "url":"http://127.0.0.1:9/mcp",
                "headers":{"Authorization":""}
            }}
        }))
        .unwrap();
        zip.write_all(&configuration).unwrap();
    }
    zip.start_file("repo-source/reminders/fixture-revision.txt", options)
        .unwrap();
    zip.write_all(version.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

fn files(
    root: &Path,
    directory: &Path,
    zip: &mut zip::ZipWriter<Cursor<Vec<u8>>>,
    version: Option<&str>,
) {
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
                format!(
                    "repo-source/{}/{relative}",
                    root.file_name().unwrap().to_str().unwrap()
                ),
                zip::write::SimpleFileOptions::default().unix_permissions(0o644),
            )
            .unwrap();
            let mut bytes = std::fs::read(path).unwrap();
            if relative == "plugin.json"
                && let Some(version) = version
            {
                let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                manifest["version"] = version.into();
                bytes = serde_json::to_vec(&manifest).unwrap();
            }
            zip.write_all(&bytes).unwrap();
        }
    }
}
