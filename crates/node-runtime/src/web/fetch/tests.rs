use super::*;
#[path = "../../../tests/support/web.rs"]
pub(super) mod server;

fn owner(server: &server::Server) -> Arc<Fetcher> {
    let owner = Arc::new(Fetcher::default());
    owner.fixture(server::HOST, server.address);
    owner
}

async fn fetch(owner: &Arc<Fetcher>, url: String) -> Result<web::Page, Fault> {
    owner
        .fetch(
            NodeId([1; 32]),
            RequestId::new(),
            &web::Request { url, offset: 0 },
            &CancellationToken::new(),
        )
        .await
}

#[tokio::test]
async fn reads_public_text_and_bounded_redirects() {
    let server = server::Server::start().await;
    let owner = owner(&server);
    let html = fetch(&owner, format!("{}/redirect/5", server.origin))
        .await
        .unwrap();
    assert_eq!(html.content, server::TEXT);
    assert_eq!(html.title.as_deref(), Some("Public page"));
    assert!(html.untrusted);
    assert_eq!(html.url, format!("{}/redirect/0", server.origin));
    let json = fetch(&owner, format!("{}/json", server.origin))
        .await
        .unwrap();
    assert_eq!(json.content, server::JSON);
    let missing = fetch(&owner, format!("{}/missing", server.origin))
        .await
        .unwrap();
    assert_eq!(missing.status, 404);
    assert_eq!(missing.content, "missing");
    for headers in server.requests.lock().unwrap().iter() {
        assert!(headers.starts_with("GET "));
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        assert!(!headers.to_ascii_lowercase().contains("cookie:"));
        assert!(!headers.to_ascii_lowercase().contains("referer:"));
    }
}

#[tokio::test]
async fn pages_cached_content_without_refetching() {
    let server = server::Server::start().await;
    let owner = owner(&server);
    let url = format!("{}/paged", server.origin);
    let first = fetch(&owner, url.clone()).await.unwrap();
    assert_eq!(first.content.chars().count(), web::PAGE_CHARS);
    assert_eq!(first.next_offset, Some(web::PAGE_CHARS));
    assert!(first.truncated);
    // Cached continuation reads make no connection, even if DNS has changed.
    owner
        .fixtures
        .lock()
        .unwrap()
        .get_mut(server::HOST)
        .unwrap()
        .resolved = vec!["127.0.0.1".parse().unwrap()];
    let second = owner
        .fetch(
            NodeId([1; 32]),
            RequestId::new(),
            &web::Request {
                url: url.clone(),
                offset: first.next_offset.unwrap(),
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!second.truncated);
    assert_eq!(second.next_offset, None);
    assert_eq!(
        format!("{}{}", first.content, second.content),
        server::paged()
    );
    let repeated = fetch(&owner, url).await.unwrap();
    assert_eq!(first, repeated);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn rejects_missing_and_expired_continuations() {
    let server = server::Server::start().await;
    let owner = owner(&server);
    let url = format!("{}/paged", server.origin);
    let request = web::Request {
        url: url.clone(),
        offset: web::PAGE_CHARS,
    };
    assert_eq!(
        owner
            .fetch(
                NodeId([1; 32]),
                RequestId::new(),
                &request,
                &CancellationToken::new()
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let first = fetch(&owner, url.clone()).await.unwrap();
    let slot = owner.cache.lock().unwrap().slot(&url, 0).unwrap();
    slot.lock().await.as_mut().unwrap().fetched -= Duration::from_secs(web::CACHE_SECS);
    assert_eq!(
        owner
            .fetch(
                NodeId([1; 32]),
                RequestId::new(),
                &request,
                &CancellationToken::new()
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    assert_eq!(fetch(&owner, url).await.unwrap(), first);
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn coalesces_concurrent_page_reads() {
    let server = server::Server::start().await;
    let owner = owner(&server);
    let url = format!("{}/paged", server.origin);
    let (first, second) = tokio::join!(fetch(&owner, url.clone()), fetch(&owner, url));
    assert_eq!(first.unwrap(), second.unwrap());
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn rejects_private_redirects_and_limits() {
    let server = server::Server::start().await;
    let owner = owner(&server);
    for (path, code) in [
        ("/private", ErrorCode::PermissionDenied),
        ("/credentials", ErrorCode::InvalidRequest),
        ("/redirect/6", ErrorCode::Unavailable),
        ("/large", ErrorCode::Unavailable),
        ("/declared", ErrorCode::Unavailable),
        ("/binary", ErrorCode::Unavailable),
    ] {
        let fault = fetch(&owner, format!("{}{path}", server.origin))
            .await
            .unwrap_err();
        assert_eq!(fault.code, code, "{path}");
        if path == "/binary" {
            assert!(fault.message.contains("image/png"));
        }
    }
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|headers| headers.starts_with("GET /html "))
            .count(),
        0
    );
}

#[tokio::test]
async fn revalidates_dns_on_redirect() {
    let server = server::Server::start().await;
    let owner = owner(&server);
    let running = {
        let owner = owner.clone();
        let url = format!("{}/rebind", server.origin);
        tokio::spawn(async move { fetch(&owner, url).await })
    };
    tokio::time::timeout(Duration::from_secs(3), server.held.cancelled())
        .await
        .unwrap();
    owner
        .fixtures
        .lock()
        .unwrap()
        .get_mut(server::HOST)
        .unwrap()
        .resolved
        .push("127.0.0.1".parse().unwrap());
    server.release.cancel();
    assert_eq!(
        running.await.unwrap().unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cancels_and_releases_requests() {
    let server = server::Server::start().await;
    let owner = owner(&server);
    let caller = NodeId([1; 32]);
    let id = RequestId::new();
    let running = {
        let owner = owner.clone();
        let url = format!("{}/hold", server.origin);
        tokio::spawn(async move {
            owner
                .fetch(
                    caller,
                    id,
                    &web::Request { url, offset: 0 },
                    &CancellationToken::new(),
                )
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(3), server.held.cancelled())
        .await
        .unwrap();
    owner.cancel(NodeId([2; 32]), id);
    assert!(!running.is_finished());
    owner.cancel(caller, id);
    assert_eq!(
        running.await.unwrap().unwrap_err().code,
        ErrorCode::Cancelled
    );
    tokio::time::timeout(Duration::from_secs(3), server.disconnected.cancelled())
        .await
        .unwrap();
    assert!(owner.active.lock().unwrap().is_empty());
}

#[tokio::test]
async fn bounds_total_request_time() {
    let server = server::Server::start().await;
    let owner = Arc::new(Fetcher {
        deadline: Duration::from_millis(100),
        ..Default::default()
    });
    owner.fixture(server::HOST, server.address);
    let fault = fetch(&owner, format!("{}/hold", server.origin))
        .await
        .unwrap_err();
    assert_eq!(fault.code, ErrorCode::Unavailable);
    assert_eq!(fault.message, "web fetch deadline exceeded");
    tokio::time::timeout(Duration::from_secs(3), server.disconnected.cancelled())
        .await
        .unwrap();
    assert!(owner.active.lock().unwrap().is_empty());
}
