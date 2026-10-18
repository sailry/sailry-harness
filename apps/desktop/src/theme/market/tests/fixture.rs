use super::super::catalog::{Entry, Resource};
use super::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};

pub(super) struct Server {
    pub service: Service,
    pub routes: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    pub requests: Arc<Mutex<Vec<String>>>,
    pub changed: Arc<tokio::sync::Notify>,
    pub held: Arc<Mutex<Option<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    pub async fn start(root: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let routes = Arc::new(Mutex::new(BTreeMap::<String, Vec<u8>>::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let changed = Arc::new(tokio::sync::Notify::new());
        let held = Arc::new(Mutex::new(None::<String>));
        let release = CancellationToken::new();
        let (values, received, notice, gate, ready) = (
            routes.clone(),
            requests.clone(),
            changed.clone(),
            held.clone(),
            release.clone(),
        );
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0u8; 1024];
                    let length = stream.read(&mut chunk).await.unwrap();
                    if length == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&chunk[..length]);
                    if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8(bytes).unwrap();
                let path = request
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                received.lock().unwrap().push(path.clone());
                notice.notify_waiters();
                let hold = gate.lock().unwrap().as_ref() == Some(&path);
                if hold {
                    ready.cancelled().await;
                }
                let body = values.lock().unwrap().get(&path).cloned();
                let status = if body.is_some() {
                    "200 OK"
                } else {
                    "404 Not Found"
                };
                let body = body.unwrap_or_default();
                let header = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if stream.write_all(header.as_bytes()).await.is_ok() {
                    let _ = stream.write_all(&body).await;
                }
            }
        });
        let mut service = Service::new(root.into()).unwrap();
        service.source = transfer::Source {
            commits: format!("http://{address}/commits/main"),
            raw: format!("http://{address}/raw/"),
        };
        Self {
            service,
            routes,
            requests,
            changed,
            held,
            task,
        }
    }

    pub fn publish(&self, source: &Path, commit: &str) -> Entry {
        let loaded = Loaded::read(source, true).unwrap();
        let paths = loaded
            .manifest
            .light
            .assets
            .values()
            .chain(loaded.manifest.dark.assets.values())
            .map(|asset| asset.path.clone())
            .chain(std::iter::once("theme.json".into()))
            .collect::<std::collections::BTreeSet<_>>();
        let resources = paths
            .iter()
            .map(|path| {
                let bytes = std::fs::read(source.join(path)).unwrap();
                self.routes
                    .lock()
                    .unwrap()
                    .insert(format!("/raw/{commit}/tide/{path}"), bytes.clone());
                Resource {
                    path: path.clone(),
                    bytes: bytes.len(),
                    digest: format!("sha256:{:x}", Sha256::digest(bytes)),
                }
            })
            .collect();
        let entry = Entry {
            id: "tide".into(),
            path: "tide".into(),
            name: loaded.manifest.name.clone(),
            package_version: loaded.manifest.package_version.clone().unwrap(),
            author: "Sailry".into(),
            license: "CC0-1.0".into(),
            description: loaded.manifest.description.clone(),
            description_locales: Default::default(),
            homepage: None,
            digest: loaded.digest(),
            resources,
            modes: vec!["light".into(), "dark".into()],
            previews: [
                ("light".into(), "assets/light.png".into()),
                ("dark".into(), "assets/dark.png".into()),
            ]
            .into(),
        };
        let mut routes = self.routes.lock().unwrap();
        routes.insert(
            "/commits/main".into(),
            serde_json::to_vec(&json!({"sha":commit})).unwrap(),
        );
        routes.insert(
            format!("/raw/{commit}/catalog.json"),
            serde_json::to_vec(&Catalog {
                version: 1,
                themes: vec![entry.clone()],
            })
            .unwrap(),
        );
        entry
    }

    pub fn publish_without_hints(&self, source: &Path, commit: &str) {
        let entry = self.publish(source, commit);
        let mut entry = serde_json::to_value(entry).unwrap();
        for name in ["resources", "modes", "previews", "description_locales"] {
            entry.as_object_mut().unwrap().remove(name);
        }
        self.routes.lock().unwrap().insert(
            format!("/raw/{commit}/catalog.json"),
            serde_json::to_vec(&json!({"version":1,"themes":[entry]})).unwrap(),
        );
    }

    pub async fn wait(&self, path: &str, count: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let changed = self.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if self
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|value| value.as_str() == path)
                    .count()
                    >= count
                {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("theme request deadline");
    }

    pub async fn offline(&mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}
