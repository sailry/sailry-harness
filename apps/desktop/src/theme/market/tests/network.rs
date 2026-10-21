use super::{FIRST, Failure, Server, Service, SourceFailure, transfer};
use sailry_link::CancellationToken;
use std::process::{Command, Output};

async fn child(service: &Service, proxy: &str, bypass: &str) -> Output {
    let (_, module) = module_path!().split_once("::").unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--ignored",
        "--exact",
        &format!("{module}::proxy_child"),
        "--nocapture",
    ]);
    for key in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ] {
        command.env_remove(key);
    }
    command
        .env("HTTP_PROXY", proxy)
        .env("NO_PROXY", bypass)
        .env("SAILRY_THEME_COMMITS", &service.source.commits)
        .env("SAILRY_THEME_RAW", &service.source.raw);
    tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap()
}

fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "proxy child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn uses_proxy_environment() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let mut proxy = Server::start(&directory.path().join("installed")).await;
    proxy.publish(&source, FIRST);
    let endpoint = proxy
        .service
        .source
        .commits
        .strip_suffix("/commits/main")
        .unwrap()
        .to_owned();
    let origin = "http://themes.invalid";
    {
        let mut routes = proxy.routes.lock().unwrap();
        *routes = std::mem::take(&mut *routes)
            .into_iter()
            .map(|(path, body)| (format!("{origin}{path}"), body))
            .collect();
    }
    proxy.service.source = transfer::Source {
        commits: format!("{origin}/commits/main"),
        raw: format!("{origin}/raw/"),
    };
    succeeded(&child(&proxy.service, &endpoint, "").await);
    assert_eq!(
        *proxy.requests.lock().unwrap(),
        [
            format!("{origin}/commits/main"),
            format!("{origin}/raw/{FIRST}/catalog.json"),
            format!("{origin}/raw/{FIRST}/tide/assets/light.png"),
        ]
    );
}

#[tokio::test]
async fn respects_proxy_exclusions() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let origin = Server::start(&directory.path().join("installed")).await;
    origin.publish(&source, FIRST);
    let proxy = Server::start(&directory.path().join("proxy")).await;
    let endpoint = proxy
        .service
        .source
        .commits
        .strip_suffix("/commits/main")
        .unwrap();
    succeeded(&child(&origin.service, endpoint, "127.0.0.1").await);
    assert!(proxy.requests.lock().unwrap().is_empty());
    assert_eq!(origin.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
#[ignore = "only invoked by proxy tests in an isolated environment"]
async fn proxy_child() {
    let directory = tempfile::tempdir().unwrap();
    let mut service = Service::new(directory.path().into()).unwrap();
    service.source = transfer::Source {
        commits: std::env::var("SAILRY_THEME_COMMITS").unwrap(),
        raw: std::env::var("SAILRY_THEME_RAW").unwrap(),
    };
    let stop = CancellationToken::new();
    let listing = service.list(false, &stop).await.unwrap();
    assert!(!listing.unavailable);
    assert_eq!(listing.commit.as_deref(), Some(FIRST));
    let selection = listing.selection("tide").unwrap();
    assert!(
        service
            .preview(&selection, "light", &stop)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn classifies_status_without_request_secrets() {
    let directory = tempfile::tempdir().unwrap();
    let server = Server::start(directory.path()).await;
    let error = transfer::body(
        &server.service.client,
        &format!("{}?token=private-request", server.service.source.commits),
        1024,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error, Failure::Unavailable(SourceFailure::Http(404)));
    assert_eq!(error.to_string(), "theme source returned HTTP 404");
    assert!(!format!("{error:?}").contains("private-request"));
}
