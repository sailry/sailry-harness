use sailry_protocol::{
    Command,
    plugin::skills::{Candidate, Discovery, Source},
};
use std::{
    io::{Cursor, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub const FIRST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const SECOND: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

pub struct Github {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    pub updated: Arc<AtomicBool>,
    worker: tokio::task::JoinHandle<()>,
}

impl Github {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let updated = Arc::new(AtomicBool::new(false));
        let current = updated.clone();
        let worker = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = socket.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        break;
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
                let (content_type, body) = if path.starts_with("/repos/owner/repo/commits/") {
                    assert!(
                        request
                            .to_ascii_lowercase()
                            .contains("accept: application/vnd.github.sha")
                    );
                    (
                        "application/vnd.github.sha",
                        if current.load(Ordering::SeqCst) {
                            SECOND
                        } else {
                            FIRST
                        }
                        .as_bytes()
                        .to_vec(),
                    )
                } else if path == format!("/owner/repo/zip/{FIRST}") {
                    ("application/zip", archive("first"))
                } else if path == format!("/owner/repo/zip/{SECOND}") {
                    ("application/zip", archive("second"))
                } else {
                    panic!("unexpected GitHub request: {request}");
                };
                captured.lock().unwrap().push(request);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(header.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
                socket.shutdown().await.unwrap();
            }
        });
        Self {
            endpoint,
            requests,
            updated,
            worker,
        }
    }
}

impl Drop for Github {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

pub fn source() -> Source {
    Source {
        repository: "Owner/Repo".into(),
        git_ref: None,
        path: None,
    }
}

pub fn install(discovery: &Discovery, skill: &Candidate, revision: u64) -> Command {
    Command::InstallSkill {
        source: discovery.source.clone(),
        path: skill.path.clone(),
        name: skill.name.clone(),
        expected_revision: revision,
    }
}

pub fn body(version: &str) -> String {
    format!(
        "---\nname: analysis\ndescription: Analyze project data\nallowed-tools:\n  - Bash(curl:*)\n  - Bash(jq:*)\n  - Read\n---\nVersion {version}\nRead references/guide.md and scripts/check.sh\nLiteral {{missing_state}} 中文 🙂\n"
    )
}

fn archive(version: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in [
        ("LICENSE", "Fixture license notice".into()),
        ("skills/analysis/SKILL.md", body(version)),
        (
            "skills/analysis/references/guide.md",
            format!("Guide {version} 中文 🙂"),
        ),
        (
            "skills/analysis/scripts/check.sh",
            format!("printf 'script {version}'; printf x >> script-count.txt\n"),
        ),
        ("skills/analysis/assets/example.txt", "Asset content".into()),
        (
            "skills/analysis/references/example/SKILL.md",
            "Example document is not an install candidate".into(),
        ),
        (
            "other/SKILL.md",
            "---\nname: writing\ndescription: Write clearly\n---\nKeep text concise\n".into(),
        ),
        (
            "unsafe/SKILL.md",
            "---\nname: unsafe\ndescription: A skill with an unavailable resource\n---\nRead references/secret\n".into(),
        ),
    ] {
        zip.start_file(
            format!("repo-source/{name}"),
            zip::write::SimpleFileOptions::default().unix_permissions(if name.ends_with(".sh") {
                0o755
            } else {
                0o644
            }),
        )
        .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    for (path, target) in [
        ("CLAUDE.md", "AGENTS.md"),
        ("unsafe/references/secret", "../../../secret"),
    ] {
        zip.add_symlink(
            format!("repo-source/{path}"),
            target,
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    }
    zip.finish().unwrap().into_inner()
}
