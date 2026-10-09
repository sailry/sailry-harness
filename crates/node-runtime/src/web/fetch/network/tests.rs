use super::super::tests::server;
use super::*;
use hyper_util::client::proxy::matcher::Matcher;
#[path = "../../../../tests/support/web_proxy.rs"]
mod proxy;
mod tls;

fn configured(host: &str, address: SocketAddr, proxy: Option<&proxy::Proxy>) -> Fetcher {
    let owner = Fetcher::default();
    owner.fixture(host, address);
    if let Some(proxy) = proxy {
        owner.fixture_proxy(
            host,
            Matcher::builder()
                .all(proxy.url.replacen("http://", "http://fixture:private@", 1))
                .build(),
        );
    }
    owner
}

fn authenticated(request: &str) {
    assert!(
        request
            .to_ascii_lowercase()
            .contains("proxy-authorization: basic zml4dhvyztpwcml2yxrl")
    );
}

#[tokio::test]
async fn delegates_hostname_resolution() {
    let server = server::Server::start().await;
    let proxy = proxy::Proxy::start(vec![server.address]).await;
    let owner = configured(server::HOST, server.address, None);
    owner.fixture_proxy(
        server::HOST,
        Matcher::builder()
            .all(proxy.url.replacen("http://", "http://fixture:private@", 1))
            .no("127.0.0.1")
            .build(),
    );
    let page = fetch(
        &owner,
        Url::parse(&format!("{}/html", server.origin)).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(page.content, server::TEXT);
    {
        let requests = proxy.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with(&format!("GET {}/html HTTP/1.1", server.origin)));
        authenticated(&requests[0]);
    }
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].to_ascii_lowercase().contains(&format!(
        "host: {}:{}",
        server::HOST,
        server.address.port()
    )));
    assert!(requests[0].starts_with("GET /html HTTP/1.1"));
    assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
    assert!(!requests[0].contains("private"));
}

#[tokio::test]
async fn preserves_tls_hostname_and_origin_headers() {
    let server = tls::Server::start().await;
    let proxy = proxy::Proxy::start(vec![server.address]).await;
    let owner = configured(tls::HOST, server.address, Some(&proxy));
    owner
        .fixtures
        .lock()
        .unwrap()
        .get_mut(tls::HOST)
        .unwrap()
        .roots
        .push(server.root.clone());
    let page = fetch(&owner, Url::parse(&server.url).unwrap())
        .await
        .unwrap();
    assert_eq!(page.content, "Secure origin");
    {
        let requests = proxy.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with(&format!(
            "CONNECT {}:{} HTTP/1.1",
            tls::HOST,
            server.address.port()
        )));
        authenticated(&requests[0]);
    }
    {
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].server_name.as_deref(), Some(tls::HOST));
        assert!(requests[0].headers.starts_with("GET /secure HTTP/1.1"));
        assert!(requests[0].headers.to_ascii_lowercase().contains(&format!(
            "host: {}:{}",
            tls::HOST,
            server.address.port()
        )));
        assert!(
            !requests[0]
                .headers
                .to_ascii_lowercase()
                .contains("authorization:")
        );
    }
    let direct = configured(tls::HOST, server.address, None);
    direct
        .fixtures
        .lock()
        .unwrap()
        .get_mut(tls::HOST)
        .unwrap()
        .roots
        .push(server.root.clone());
    assert_eq!(
        fetch(&direct, Url::parse(&server.url).unwrap())
            .await
            .unwrap()
            .content,
        "Secure origin"
    );
    assert_eq!(proxy.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn rechecks_redirects_before_proxy_connections() {
    let server = server::Server::start().await;
    let proxy = proxy::Proxy::start(vec![server.address]).await;
    let owner = configured(server::HOST, server.address, Some(&proxy));
    assert_eq!(
        fetch(
            &owner,
            Url::parse(&format!("{}/redirect/2", server.origin)).unwrap()
        )
        .await
        .unwrap()
        .content,
        server::TEXT
    );
    assert_eq!(proxy.requests.lock().unwrap().len(), 3);
    assert_eq!(
        fetch(
            &owner,
            Url::parse(&format!("{}/private", server.origin)).unwrap()
        )
        .await
        .err()
        .expect("private redirect must fail")
        .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(proxy.requests.lock().unwrap().len(), 4);
    assert_eq!(server.requests.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn tolerates_fake_ips_and_unavailable_dns() {
    for resolved in [vec!["198.18.0.7".parse().unwrap()], Vec::new()] {
        let server = server::Server::start().await;
        let proxy = proxy::Proxy::start(vec![server.address]).await;
        let owner = configured(server::HOST, server.address, Some(&proxy));
        owner
            .fixtures
            .lock()
            .unwrap()
            .get_mut(server::HOST)
            .unwrap()
            .resolved = resolved;
        assert_eq!(
            fetch(
                &owner,
                Url::parse(&format!("{}/html", server.origin)).unwrap()
            )
            .await
            .unwrap()
            .content,
            server::TEXT
        );
        assert_eq!(proxy.requests.lock().unwrap().len(), 1);
        let direct = configured(server::HOST, server.address, None);
        direct
            .fixtures
            .lock()
            .unwrap()
            .get_mut(server::HOST)
            .unwrap()
            .resolved = vec!["198.18.0.7".parse().unwrap()];
        assert_eq!(
            fetch(
                &direct,
                Url::parse(&format!("{}/html", server.origin)).unwrap()
            )
            .await
            .err()
            .unwrap()
            .code,
            ErrorCode::PermissionDenied
        );
    }
}

#[tokio::test]
async fn rejects_private_answers_before_proxy_access() {
    let server = server::Server::start().await;
    let proxy = proxy::Proxy::start(vec![server.address]).await;
    let owner = configured(server::HOST, server.address, Some(&proxy));
    owner
        .fixtures
        .lock()
        .unwrap()
        .get_mut(server::HOST)
        .unwrap()
        .resolved = vec!["127.0.0.1".parse().unwrap()];
    assert_eq!(
        fetch(
            &owner,
            Url::parse(&format!("{}/html", server.origin)).unwrap()
        )
        .await
        .err()
        .unwrap()
        .code,
        ErrorCode::PermissionDenied
    );
    assert!(proxy.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn preserves_original_hostname_exclusions() {
    let server = server::Server::start().await;
    let proxy = proxy::Proxy::start(vec![server.address]).await;
    let owner = configured(server::HOST, server.address, None);
    owner.fixture_proxy(
        server::HOST,
        Matcher::builder()
            .all(proxy.url.replacen("http://", "http://fixture:private@", 1))
            .no(server::HOST)
            .build(),
    );
    assert_eq!(
        fetch(
            &owner,
            Url::parse(&format!("{}/html", server.origin)).unwrap()
        )
        .await
        .unwrap()
        .content,
        server::TEXT
    );
    assert!(proxy.requests.lock().unwrap().is_empty());
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
    assert!(requests[0].to_ascii_lowercase().contains(&format!(
        "host: {}:{}",
        server::HOST,
        server.address.port()
    )));
}

#[tokio::test]
async fn keeps_default_forwarding_unchanged() {
    use tokio::io::AsyncWriteExt;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let capture = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = proxy::header(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\ndefault")
            .await
            .unwrap();
        request
    });
    let client = reqwest::Client::builder()
        .proxy(
            reqwest::Proxy::all(endpoint)
                .unwrap()
                .basic_auth("fixture", "private"),
        )
        .build()
        .unwrap();
    assert_eq!(
        client
            .get("http://unchanged.sailry.test:1234/path")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "default"
    );
    let request = capture.await.unwrap();
    assert!(request.starts_with("GET http://unchanged.sailry.test:1234/path HTTP/1.1"));
    authenticated(&request);
}
