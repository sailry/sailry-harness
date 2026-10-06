use super::{super::transfer, fixture};
use sailry_link::CancellationToken;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

pub(in crate::updater) async fn server(
    contents: impl FnOnce(&str) -> BTreeMap<String, Vec<u8>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let (base, _, task) = responses(|base| {
        contents(base)
            .into_iter()
            .map(|(path, bytes)| (path, Response::Body(bytes)))
            .collect()
    })
    .await;
    (base, task)
}

enum Response {
    Body(Vec<u8>),
    Redirect(String),
}

async fn responses(
    contents: impl FnOnce(&str) -> BTreeMap<String, Response>,
) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let contents = contents(&base);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let received = requests.clone();
    let task = tokio::spawn(async move {
        for _ in 0..contents.len() {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let length = stream.read(&mut request).await.unwrap();
            let request = std::str::from_utf8(&request[..length]).unwrap();
            received.lock().unwrap().push(request.to_owned());
            let path = request.split_whitespace().nth(1).unwrap();
            let (header, bytes) = match &contents[path] {
                Response::Body(bytes) => (
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        bytes.len()
                    ),
                    bytes.as_slice(),
                ),
                Response::Redirect(location) => (
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    ),
                    &[][..],
                ),
            };
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(bytes).await.unwrap();
        }
    });
    (base, requests, task)
}

mod redirects {
    use super::*;

    #[test]
    fn validates_destinations_and_disallows_https_downgrades() {
        let initial = url::Url::parse("https://example.test/update.zip").unwrap();
        for target in [
            "http://example.test/update.zip",
            "http://127.0.0.1/update.zip",
            "ftp://127.0.0.1/update.zip",
            "https://user:credential@example.test/update.zip",
            "https://example.test/update.zip#fragment",
        ] {
            assert!(
                transfer::redirect(
                    &url::Url::parse(target).unwrap(),
                    std::slice::from_ref(&initial)
                )
                .is_err()
            );
        }
        assert!(
            transfer::redirect(
                &url::Url::parse("https://self-hosted.test/update.zip?signature=fixture").unwrap(),
                &[initial],
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn authenticates_redirected_metadata_without_forwarding_a_referer() {
        let bytes = fixture::signed(&[fixture::release()]);
        let (base, requests, serving) = responses(|base| {
            BTreeMap::from([
                (
                    "/start?signature=fixture".into(),
                    Response::Redirect(format!("{base}/update.json")),
                ),
                ("/update.json".into(), Response::Body(bytes)),
            ])
        })
        .await;
        let mut config = fixture::config();
        config.source = format!("{base}/start?signature=fixture");
        let selection = transfer::check(
            transfer::client().unwrap(),
            &config,
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(selection.release.version, "9.9.9");
        serving.await.unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| !request.to_ascii_lowercase().contains("\r\nreferer:"))
        );
    }

    #[tokio::test]
    async fn rejects_unsafe_destinations_before_fetching() {
        for credentials in [false, true] {
            let (base, requests, serving) = responses(|base| {
                let target = if credentials {
                    base.replace("http://", "http://user:credential@")
                } else {
                    base.replace("http://", "ftp://")
                };
                BTreeMap::from([
                    (
                        "/start".into(),
                        Response::Redirect(format!("{target}/blocked")),
                    ),
                    (
                        "/blocked".into(),
                        Response::Body(fixture::signed(&[fixture::release()])),
                    ),
                ])
            })
            .await;
            let mut config = fixture::config();
            config.source = format!("{base}/start");
            assert!(
                transfer::check(
                    transfer::client().unwrap(),
                    &config,
                    &CancellationToken::new(),
                )
                .await
                .is_err()
            );
            serving.abort();
            assert_eq!(requests.lock().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn rejects_loops_before_repeating_a_request() {
        let (base, requests, serving) = responses(|base| {
            BTreeMap::from([
                (
                    "/start".into(),
                    Response::Redirect(format!("{base}/second")),
                ),
                (
                    "/second".into(),
                    Response::Redirect(format!("{base}/start")),
                ),
            ])
        })
        .await;
        let mut config = fixture::config();
        config.source = format!("{base}/start");
        assert!(
            transfer::check(
                transfer::client().unwrap(),
                &config,
                &CancellationToken::new(),
            )
            .await
            .is_err()
        );
        serving.await.unwrap();
        assert_eq!(requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn follows_at_most_five_hops() {
        let (base, requests, serving) = responses(|base| {
            let mut responses = (0..7)
                .map(|index| {
                    (
                        format!("/{index}"),
                        Response::Redirect(format!("{base}/{}", index + 1)),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            responses.insert(
                "/7".into(),
                Response::Body(fixture::signed(&[fixture::release()])),
            );
            responses
        })
        .await;
        let mut config = fixture::config();
        config.source = format!("{base}/0");
        assert!(
            transfer::check(
                transfer::client().unwrap(),
                &config,
                &CancellationToken::new(),
            )
            .await
            .is_err()
        );
        serving.abort();
        assert_eq!(requests.lock().unwrap().len(), 6);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn requires_complete_bundle_verification() {
        let staged = fixture::staged();
        let bytes = std::fs::read(&staged.archive).unwrap();
        let (base, requests, serving) = responses(|base| {
            BTreeMap::from([
                (
                    "/release.zip".into(),
                    Response::Redirect(format!("{base}/asset.zip")),
                ),
                ("/asset.zip".into(), Response::Body(bytes)),
            ])
        })
        .await;
        let mut release = staged.selection.release.clone();
        release.url = format!("{base}/release.zip");
        let selection = super::super::super::manifest::Selection {
            manifest: fixture::signed(&[release.clone()]),
            release,
        };
        let directory = tempfile::tempdir().unwrap();
        let staged = transfer::download(
            transfer::client().unwrap(),
            fixture::config(),
            selection,
            directory.path(),
            CancellationToken::new(),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
        assert!(staged.archive.is_file());
        assert!(
            staged
                .directory
                .path()
                .join("contents/Sailry.app/Contents/Resources/build.json")
                .is_file()
        );
        serving.await.unwrap();
        assert_eq!(requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn verifies_digest_and_publisher_after_a_redirect() {
        for expected in ["updates_digest_invalid", "updates_signature_invalid"] {
            let mut release = fixture::release();
            let mut bytes = fixture::archive(&[("fixture", b"isolated archive")], &release.name);
            release.sha256 = fixture::digest(&bytes);
            bytes[0] ^= 1;
            if expected == "updates_signature_invalid" {
                release.sha256 = fixture::digest(&bytes);
            }
            release.size = bytes.len() as u64;
            let (base, requests, serving) = responses(|base| {
                BTreeMap::from([
                    (
                        "/release.zip".into(),
                        Response::Redirect(format!("{base}/asset.zip")),
                    ),
                    ("/asset.zip".into(), Response::Body(bytes)),
                ])
            })
            .await;
            release.url = format!("{base}/release.zip");
            let selection = super::super::super::manifest::Selection {
                manifest: fixture::signed(&[release.clone()]),
                release,
            };
            let directory = tempfile::tempdir().unwrap();
            let result = transfer::download(
                transfer::client().unwrap(),
                fixture::config(),
                selection,
                directory.path(),
                CancellationToken::new(),
                Arc::new(|_| {}),
            )
            .await;
            assert_eq!(result.err().unwrap().key, expected);
            serving.await.unwrap();
            assert_eq!(requests.lock().unwrap().len(), 2);
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        }
    }
}

#[tokio::test]
async fn check_authenticates_injected_transport_response() {
    let bytes = fixture::signed(&[fixture::release()]);
    let (base, serving) = server(|_| BTreeMap::from([("/update.json".into(), bytes)])).await;
    let mut config = fixture::config();
    config.source = format!("{base}/update.json");
    let client = transfer::client().unwrap();
    let selection = transfer::check(client, &config, &CancellationToken::new())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(selection.release.version, "9.9.9");
    serving.await.unwrap();
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn downloads_verifies_and_reports_ready_without_installing() {
    let staged = fixture::staged();
    let bytes = std::fs::read(&staged.archive).unwrap();
    let (base, serving) = server(|_| BTreeMap::from([("/package.zip".into(), bytes)])).await;
    let mut release = staged.selection.release.clone();
    release.url = format!("{base}/package.zip");
    let selection = super::super::manifest::Selection {
        manifest: fixture::signed(&[release.clone()]),
        release,
    };
    let directory = tempfile::tempdir().unwrap();
    let progress = Arc::new(Mutex::new(Vec::new()));
    let changed = progress.clone();
    let client = transfer::client().unwrap();
    let result = transfer::download(
        client,
        fixture::config(),
        selection,
        directory.path(),
        CancellationToken::new(),
        Arc::new(move |value| changed.lock().unwrap().push(value)),
    )
    .await
    .unwrap();
    assert!(result.archive.is_file());
    assert!(
        result
            .directory
            .path()
            .join("contents/Sailry.app/Contents/Resources/build.json")
            .is_file()
    );
    assert!(
        progress
            .lock()
            .unwrap()
            .contains(&transfer::Progress::Verifying)
    );
    assert!(progress.lock().unwrap().iter().any(|value| matches!(value, transfer::Progress::Downloading { copied, total: Some(total) } if copied == total)));
    serving.await.unwrap();
}

#[tokio::test]
async fn cancellation_cleans_only_its_partial_download() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/package.zip", listener.local_addr().unwrap());
    let (started, received) = tokio::sync::oneshot::channel();
    let serving = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(stream.read(&mut request).await.unwrap() > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1048576\r\n\r\npartial")
            .await
            .unwrap();
        let _ = started.send(());
        std::future::pending::<()>().await;
    });
    let mut release = fixture::release();
    release.url = url;
    release.size = 1048576;
    let selection = super::super::manifest::Selection {
        manifest: fixture::signed(&[release.clone()]),
        release,
    };
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("unrelated"), b"preserve").unwrap();
    let stop = CancellationToken::new();
    let cancelling = stop.clone();
    let client = transfer::client().unwrap();
    let download = transfer::download(
        client,
        fixture::config(),
        selection,
        directory.path(),
        stop,
        Arc::new(|_| {}),
    );
    let cancel = async move {
        received.await.unwrap();
        cancelling.cancel();
    };
    let (result, ()) = tokio::join!(download, cancel);
    assert_eq!(result.err().unwrap().key, "updates_cancelled");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    assert_eq!(
        std::fs::read(directory.path().join("unrelated")).unwrap(),
        b"preserve"
    );
    serving.abort();
}
