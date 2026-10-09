use super::*;
use reqwest::{Client, Url};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

enum Reply {
    Body(Vec<u8>),
    Chunks(Vec<Vec<u8>>, Duration),
    Stall,
}

struct Server {
    url: Url,
    requests: Arc<AtomicUsize>,
    paths: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl Server {
    async fn start(reply: Reply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        let paths = Arc::new(Mutex::new(Vec::new()));
        let recorded = paths.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if socket.read(&mut byte).await.unwrap() == 0 {
                        break;
                    }
                    request.extend_from_slice(&byte);
                    assert!(request.len() < 4096);
                }
                let path = std::str::from_utf8(&request)
                    .unwrap()
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                recorded.lock().unwrap().push(path);
                counter.fetch_add(1, Ordering::SeqCst);
                match &reply {
                    Reply::Body(body) => {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        if socket.write_all(header.as_bytes()).await.is_ok() {
                            let _ = socket.write_all(body).await;
                        }
                    }
                    Reply::Chunks(chunks, delay) => {
                        let header = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                        if socket.write_all(header).await.is_err() {
                            continue;
                        }
                        for chunk in chunks {
                            tokio::time::sleep(*delay).await;
                            let header = format!("{:x}\r\n", chunk.len());
                            if socket.write_all(header.as_bytes()).await.is_err() {
                                break;
                            }
                            if socket.write_all(chunk).await.is_err() {
                                break;
                            }
                            if socket.write_all(b"\r\n").await.is_err() {
                                break;
                            }
                        }
                        let _ = socket.write_all(b"0\r\n\r\n").await;
                    }
                    Reply::Stall => {
                        let header =
                            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n";
                        socket.write_all(header).await.unwrap();
                        std::future::pending::<()>().await;
                    }
                }
            }
        });
        Self {
            url,
            requests,
            paths,
            task,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn directory() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
}

fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn client(timeout: Duration) -> Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(1))
        .read_timeout(timeout)
        .build()
        .unwrap()
}

fn downloads<'a>(server: &Server, files: &[File<'a>]) -> Vec<Download<'a>> {
    files
        .iter()
        .map(|file| Download {
            file: *file,
            url: server.url.join(file.name).unwrap(),
        })
        .collect()
}

async fn download(
    path: &Path,
    server: &Server,
    files: &[File<'_>],
    cancel: &CancellationToken,
    progress: impl FnMut(Progress),
) -> Result<std::path::PathBuf> {
    transfer::acquire(
        path,
        &downloads(server, files),
        cancel,
        progress,
        &client(Duration::from_secs(1)),
    )
    .await
}

#[tokio::test]
async fn independent_urls_preserve_flat_targets() {
    let root = directory();
    let path = root.path().join("model");
    let first = Server::start(Reply::Body(b"first".to_vec())).await;
    let second = Server::start(Reply::Body(b"second".to_vec())).await;
    let first_hash = checksum(b"first");
    let second_hash = checksum(b"second");
    let mut second_url = second
        .url
        .join("/other/repository/resolve/pinned/nested/tokenizer.json")
        .unwrap();
    second_url.set_host(Some("localhost")).unwrap();
    let downloads = [
        Download {
            file: File {
                name: "weights.onnx",
                size: 5,
                sha256: &first_hash,
            },
            url: first
                .url
                .join("/first/repository/resolve/pinned/onnx/model.onnx")
                .unwrap(),
        },
        Download {
            file: File {
                name: "vocabulary.json",
                size: 6,
                sha256: &second_hash,
            },
            url: second_url,
        },
    ];
    assert_ne!(downloads[0].url.host_str(), downloads[1].url.host_str());
    transfer::acquire(
        &path,
        &downloads,
        &CancellationToken::new(),
        |_| {},
        &client(Duration::from_secs(1)),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(path.join("weights.onnx")).unwrap(), b"first");
    assert_eq!(
        std::fs::read(path.join("vocabulary.json")).unwrap(),
        b"second"
    );
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 2);
    assert_eq!(
        first.paths.lock().unwrap().as_slice(),
        ["/first/repository/resolve/pinned/onnx/model.onnx"]
    );
    assert_eq!(
        second.paths.lock().unwrap().as_slice(),
        ["/other/repository/resolve/pinned/nested/tokenizer.json"]
    );
    let files: Vec<_> = downloads.iter().map(|download| download.file).collect();
    assert!(ready(&path, &files));
}

#[test]
fn rejects_partial_and_changed_files() {
    let directory = directory();
    let path = directory.path().join("model");
    let checksum = checksum(b"model");
    let files = [File {
        name: "model",
        size: 5,
        sha256: &checksum,
    }];
    for (bytes, valid) in [
        (b"mod".as_slice(), false),
        (b"other".as_slice(), false),
        (b"model".as_slice(), true),
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(ready(directory.path(), &files), valid);
    }
}

#[test]
fn excludes_cached_download_bytes() {
    let progress = super::progress(750, 1000, 250, Duration::from_secs(2));
    assert_eq!(progress.percent, 75.);
    assert_eq!(progress.bytes_per_second, 125.);
    assert_eq!(super::progress(1000, 1000, 0, Duration::ZERO).percent, 99.);
    assert_eq!(
        super::progress(1000, 1000, 0, Duration::ZERO).bytes_per_second,
        0.
    );
}

#[tokio::test]
async fn validates_cache_and_reports_network_progress() {
    let directory = directory();
    std::fs::write(directory.path().join("cached"), b"cached").unwrap();
    let cached = checksum(b"cached");
    let fresh = checksum(b"net");
    let files = [
        File {
            name: "cached",
            size: 6,
            sha256: &cached,
        },
        File {
            name: "fresh",
            size: 3,
            sha256: &fresh,
        },
    ];
    let server = Server::start(Reply::Body(b"net".to_vec())).await;
    let mut progress = Vec::new();
    download(
        directory.path(),
        &server,
        &files,
        &CancellationToken::new(),
        |value| progress.push(value),
    )
    .await
    .unwrap();
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    assert_eq!(progress.first().unwrap().bytes_per_second, 0.);
    assert!(progress.iter().any(|value| value.bytes_per_second > 0.));
    assert!(
        progress
            .windows(2)
            .all(|values| values[0].percent <= values[1].percent)
    );
    assert_eq!(
        *progress.last().unwrap(),
        Progress {
            percent: 100.,
            bytes_per_second: 0.
        }
    );
    assert!(ready(directory.path(), &files));
    progress.clear();
    download(
        directory.path(),
        &server,
        &files,
        &CancellationToken::new(),
        |value| progress.push(value),
    )
    .await
    .unwrap();
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    assert!(progress.iter().all(|value| value.bytes_per_second == 0.));
    std::fs::write(directory.path().join("fresh"), b"bad").unwrap();
    assert!(!ready(directory.path(), &files));
    download(
        directory.path(),
        &server,
        &files,
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        std::fs::read(directory.path().join("fresh")).unwrap(),
        b"net"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(directory.path().join("fresh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
}

#[tokio::test]
async fn never_publishes_bad_hash_or_size() {
    let expected = checksum(b"model");
    let files = [File {
        name: "model",
        size: 5,
        sha256: &expected,
    }];
    for response in [
        Reply::Body(b"other".to_vec()),
        Reply::Body(b"oversized".to_vec()),
        Reply::Chunks(vec![b"oversized".to_vec()], Duration::ZERO),
    ] {
        let directory = directory();
        std::fs::write(directory.path().join("model"), b"previous").unwrap();
        let server = Server::start(response).await;
        assert_eq!(
            download(
                directory.path(),
                &server,
                &files,
                &CancellationToken::new(),
                |_| {}
            )
            .await,
            Err(Error::InvalidAsset)
        );
        assert_eq!(
            std::fs::read(directory.path().join("model")).unwrap(),
            b"previous"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[tokio::test]
async fn cancellation_preserves_the_existing_asset() {
    let directory = directory();
    std::fs::write(directory.path().join("model"), b"previous").unwrap();
    let expected = checksum(b"model");
    let files = [File {
        name: "model",
        size: 5,
        sha256: &expected,
    }];
    let server = Server::start(Reply::Chunks(
        vec![b"mo".to_vec(), b"del".to_vec()],
        Duration::from_millis(20),
    ))
    .await;
    let cancel = CancellationToken::new();
    let mut progress = Vec::new();
    let result = download(directory.path(), &server, &files, &cancel, |value| {
        progress.push(value);
        if value.bytes_per_second > 0. {
            cancel.cancel();
        }
    })
    .await;
    assert_eq!(result, Err(Error::Cancelled));
    assert!(progress.iter().all(|value| value.percent < 100.));
    assert_eq!(
        std::fs::read(directory.path().join("model")).unwrap(),
        b"previous"
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn inactivity_times_out_without_publishing() {
    let directory = directory();
    let expected = checksum(b"model");
    let files = [File {
        name: "model",
        size: 5,
        sha256: &expected,
    }];
    let server = Server::start(Reply::Stall).await;
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        transfer::acquire(
            directory.path(),
            &downloads(&server, &files),
            &CancellationToken::new(),
            |_| {},
            &client(Duration::from_millis(100)),
        ),
    )
    .await
    .unwrap();
    assert_eq!(result, Err(Error::Download));
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn active_streams_outlive_the_read_timeout() {
    let directory = directory();
    let expected = checksum(b"model");
    let files = [File {
        name: "model",
        size: 5,
        sha256: &expected,
    }];
    let server = Server::start(Reply::Chunks(
        b"model".iter().map(|byte| vec![*byte]).collect(),
        Duration::from_millis(300),
    ))
    .await;
    let started = Instant::now();
    transfer::acquire(
        directory.path(),
        &downloads(&server, &files),
        &CancellationToken::new(),
        |_| {},
        &client(Duration::from_secs(1)),
    )
    .await
    .unwrap();
    assert!(started.elapsed() > Duration::from_secs(1));
    assert!(ready(directory.path(), &files));
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn accepts_apple_system_aliases() {
    let checksum = checksum(b"model");
    let files = [File {
        name: "model",
        size: 5,
        sha256: &checksum,
    }];
    let server = Server::start(Reply::Body(b"model".to_vec())).await;
    for (private, alias, temporary) in [
        ("/private/var", "/var", "/private/var/tmp"),
        ("/private/tmp", "/tmp", "/private/tmp"),
    ] {
        let root = tempfile::tempdir_in(temporary).unwrap();
        let path = Path::new(alias)
            .join(root.path().strip_prefix(private).unwrap())
            .join("cache");
        download(&path, &server, &files, &CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert!(ready(&path, &files));
        assert_eq!(
            std::fs::read(root.path().join("cache/model")).unwrap(),
            b"model"
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.path().join("cache"))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

#[cfg(unix)]
#[tokio::test]
async fn refuses_symlinked_directories_and_files() {
    use std::os::unix::fs::symlink;
    let root = directory();
    let outside = directory();
    let target = outside.path().join("model");
    std::fs::write(&target, b"model").unwrap();
    symlink(outside.path(), root.path().join("linked")).unwrap();
    symlink(&target, root.path().join("model")).unwrap();
    let checksum = checksum(b"model");
    let files = [File {
        name: "model",
        size: 5,
        sha256: &checksum,
    }];
    let server = Server::start(Reply::Body(b"model".to_vec())).await;
    for path in [root.path().join("linked"), root.path().to_owned()] {
        assert!(!ready(&path, &files));
        assert_eq!(
            download(&path, &server, &files, &CancellationToken::new(), |_| {}).await,
            Err(Error::Storage)
        );
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(target).unwrap(), b"model");
}

#[test]
fn rejects_unsafe_names_and_duplicate_targets() {
    let checksum = checksum(b"model");
    for name in [
        "",
        "../model",
        "/model",
        "a/b",
        "a\\b",
        "model:stream",
        "CON",
        "COM1.bin",
        "model.",
    ] {
        assert_eq!(
            validate(&[File {
                name,
                size: 5,
                sha256: &checksum
            }]),
            Err(Error::InvalidAsset)
        );
    }
    assert_eq!(
        validate(&[
            File {
                name: "model",
                size: 5,
                sha256: &checksum
            },
            File {
                name: "MODEL",
                size: 5,
                sha256: &checksum
            }
        ]),
        Err(Error::InvalidAsset)
    );
}

#[tokio::test]
async fn cancellation_and_bad_sources_avoid_storage() {
    let root = directory();
    let path = root.path().join("new");
    let checksum = checksum(b"model");
    let file = File {
        name: "model",
        size: 5,
        sha256: &checksum,
    };
    let valid = Download {
        file,
        url: "https://example.invalid/model".parse().unwrap(),
    };
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        prepare(&path, std::slice::from_ref(&valid), &cancel, |_| {}).await,
        Err(Error::Cancelled)
    );
    for source in [
        "http://127.0.0.1/model",
        "https://user:secret@example.invalid/model",
        "https://example.invalid/model#fragment",
        "https://user@example.invalid/model",
        "ftp://example.invalid/model",
        "file:///model",
    ] {
        let invalid = Download {
            file: File {
                name: "second",
                ..file
            },
            url: source.parse().unwrap(),
        };
        assert_eq!(
            prepare(
                &path,
                &[valid.clone(), invalid],
                &CancellationToken::new(),
                |_| {}
            )
            .await,
            Err(Error::InvalidAsset)
        );
        assert!(!path.exists());
    }
    assert!(!path.exists());
}

#[tokio::test]
async fn accepts_queries_for_pinned_assets() {
    let directory = directory();
    std::fs::write(directory.path().join("model"), b"model").unwrap();
    let checksum = checksum(b"model");
    let downloads = [Download {
        file: File {
            name: "model",
            size: 5,
            sha256: &checksum,
        },
        url: "https://example.invalid/repo?Revision=fixed&FilePath=model"
            .parse()
            .unwrap(),
    }];
    prepare(
        directory.path(),
        &downloads,
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(directory.path().join("model")).unwrap(),
        b"model"
    );
}
