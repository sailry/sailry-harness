use sailry_node_runtime::CodeServerFixture;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

pub(crate) struct Network {
    pub endpoint: String,
    pub asset: CodeServerFixture,
    pub held: Arc<AtomicBool>,
    pub hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    children: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Network {
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let asset = CodeServerFixture::new();
        let python = std::process::Command::new("python3")
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap();
        assert!(python.status.success());
        let python = PathBuf::from(String::from_utf8(python.stdout).unwrap().trim());
        let archive = CodeServerFixture::archive(python);
        let asset_name = asset.asset_url.rsplit('/').next().unwrap();
        let release=serde_json::to_vec(&json!({"assets":[{"name":asset_name,"digest":format!("sha256:{:x}",Sha256::digest(&archive))}]})).unwrap();
        let mut routes = BTreeMap::from([
            (asset.asset_path(), archive),
            (asset.release_path(), release),
        ]);
        rules(&mut routes);
        let routes = Arc::new(routes);
        let held = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let children = Arc::new(Mutex::new(Vec::new()));
        let thread = Some(std::thread::spawn({
            let held = held.clone();
            let hits = hits.clone();
            let stop = stop.clone();
            let children = children.clone();
            let asset_path = asset.asset_path();
            move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let routes = routes.clone();
                            let held = held.clone();
                            let hits = hits.clone();
                            let stop = stop.clone();
                            let asset_path = asset_path.clone();
                            children.lock().unwrap().push(std::thread::spawn(move || {
                                serve(stream, &routes, &asset_path, &held, &hits, &stop)
                            }));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) => panic!("code fixture listener: {error}"),
                    }
                }
            }
        }));
        Self {
            endpoint,
            asset,
            held,
            hits,
            stop,
            thread,
            children,
        }
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.held.store(false, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
        for child in self.children.lock().unwrap().drain(..) {
            child.join().unwrap();
        }
    }
}

fn serve(
    mut stream: TcpStream,
    routes: &BTreeMap<String, Vec<u8>>,
    asset_path: &str,
    held: &AtomicBool,
    hits: &AtomicUsize,
    stop: &AtomicBool,
) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    while bytes.len() < 32 * 1024 && !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let Ok(count) = stream.read(&mut chunk) else {
            return;
        };
        if count == 0 {
            return;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let request = String::from_utf8_lossy(&bytes);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default();
    if path == asset_path {
        hits.fetch_add(1, Ordering::SeqCst);
        while held.load(Ordering::SeqCst) && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let (status, body) = routes
        .get(path)
        .map_or(("404 Not Found", b"missing".as_slice()), |body| {
            ("200 OK", body.as_slice())
        });
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

fn rules(routes: &mut BTreeMap<String, Vec<u8>>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/node-runtime/src/code/upstream");
    let source: Value =
        serde_json::from_slice(&std::fs::read(root.join("SOURCE.json")).unwrap()).unwrap();
    let helix = source["helix"]["commit"].as_str().unwrap();
    let mason = source["mason"]["commit"].as_str().unwrap();
    let tag = source["mason"]["tag"].as_str().unwrap();
    let registry = std::fs::read(root.join("registry.json.zip")).unwrap();
    let checksums = format!("{:x}  registry.json.zip\n", Sha256::digest(&registry));
    routes.insert(
        "/api.github.com/repos/helix-editor/helix/commits/master".into(),
        serde_json::to_vec(&json!({"sha":helix})).unwrap(),
    );
    routes.insert(
        "/api.github.com/repos/mason-org/mason-registry/releases/latest".into(),
        serde_json::to_vec(&json!({"tag_name":tag})).unwrap(),
    );
    routes.insert(
        format!("/api.github.com/repos/mason-org/mason-registry/git/ref/tags/{tag}"),
        serde_json::to_vec(&json!({"object":{"type":"commit","sha":mason}})).unwrap(),
    );
    routes.insert(
        format!("/github.com/mason-org/mason-registry/releases/download/{tag}/registry.json.zip"),
        registry,
    );
    routes.insert(
        format!("/github.com/mason-org/mason-registry/releases/download/{tag}/checksums.txt"),
        checksums.into_bytes(),
    );
    routes.insert(
        format!("/raw.githubusercontent.com/helix-editor/helix/{helix}/languages.toml"),
        std::fs::read(root.join("languages.toml")).unwrap(),
    );
    routes.insert(
        format!("/raw.githubusercontent.com/helix-editor/helix/{helix}/LICENSE"),
        std::fs::read(root.join("helix-LICENSE")).unwrap(),
    );
    routes.insert(
        format!("/raw.githubusercontent.com/mason-org/mason-registry/{mason}/LICENSE"),
        std::fs::read(root.join("mason-LICENSE")).unwrap(),
    );
}
