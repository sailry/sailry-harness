use super::*;
use futures::StreamExt;
use rmcp::transport::streamable_http_client::{StreamableHttpClient, StreamableHttpPostResponse};
use sailry_protocol::plugin::settings::Binding;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
#[path = "../../../tests/support/application_tokens.rs"]
mod issuer;

fn target(provider: Provider, secret: &str) -> Target {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let binding: Binding = serde_json::from_value(match provider {
        Provider::Feishu => json!({"server":"feishu","header":"X-Lark-MCP-TAT","application":{"provider":"feishu","client_id":"app_id"}}),
        Provider::Dingtalk => json!({"origin":"https://api.dingtalk.com","header":"x-acs-dingtalk-access-token","application":{"provider":"dingtalk","client_id":"app_id"}}),
    }).unwrap();
    let settings = super::super::settings::Resolved {
        values: BTreeMap::from([("app_id".into(), json!("fixture-app"))]),
        secrets: BTreeMap::new(),
        authorizations: BTreeMap::new(),
        mcp: None,
        github: None,
    };
    Target::resolve(
        &Reference {
            name: "fixture".into(),
            digest: "capture".into(),
            settings_revision: 1,
        },
        &binding,
        &settings,
        match provider {
            Provider::Feishu => "https://mcp.feishu.cn/mcp",
            Provider::Dingtalk => "https://api.dingtalk.com/probe",
        },
        &Secret::new(secret.into()),
    )
    .unwrap()
}

#[test]
fn defers_http_initialization() {
    let host = super::super::Host::new(None);
    assert!(host.applications.client.get().is_none());
}

#[tokio::test]
async fn shares_refresh_and_preserves_expiry() {
    for provider in [Provider::Feishu, Provider::Dingtalk] {
        let server = issuer::Server::start(provider, None).await;
        let service = Service::default();
        service.fixture(provider, &server.token_url, &server.origin);
        let target = target(provider, issuer::SECRET);
        assert!(service.client.get().is_none());
        let first = service.access(&target).await.unwrap();
        assert!(service.client.get().is_some());
        assert_eq!(first.to_str().unwrap(), "application-token-1");
        assert!(first.is_sensitive());
        assert_eq!(service.access(&target).await.unwrap(), first);
        tokio::time::sleep(Duration::from_millis(2100)).await;
        let (left, right) = tokio::join!(service.access(&target), service.access(&target));
        assert_eq!(left.unwrap().to_str().unwrap(), "application-token-2");
        assert_eq!(right.unwrap().to_str().unwrap(), "application-token-2");
        assert_eq!(server.tokens.load(Ordering::SeqCst), 2);
        assert_eq!(server.records.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn keeps_provider_failures_private() {
    let server = issuer::Server::start(Provider::Feishu, None).await;
    server.failed.store(true, Ordering::SeqCst);
    let service = Service::default();
    service.fixture(Provider::Feishu, &server.token_url, &server.origin);
    let target = target(Provider::Feishu, issuer::SECRET);
    let error = service.access(&target).await.unwrap_err();
    assert_eq!(error.message, "application token request failed (HTTP 503)");
    assert!(!format!("{error:?}").contains(issuer::SECRET));
    assert_eq!(server.tokens.load(Ordering::SeqCst), 1);
    assert_eq!(
        service
            .resource(&target, "https://example.com/mcp")
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
}

#[tokio::test]
async fn forwards_sse_size_bounds() {
    let server = issuer::Server::start(Provider::Dingtalk, None).await;
    let service = Arc::new(Service::default());
    service.fixture(Provider::Dingtalk, &server.token_url, &server.origin);
    let authorization =
        Authorization::new(service, vec![target(Provider::Dingtalk, issuer::SECRET)]).unwrap();
    let client = authorization.client(reqwest::Client::new());
    let uri: Arc<str> = Arc::from("https://api.dingtalk.com/oversized");
    let mut allowed = client
        .get_stream_with_max_sse_event_size(uri.clone(), None, None, None, HashMap::new(), 4096)
        .await
        .unwrap();
    assert!(allowed.next().await.unwrap().is_ok());
    let mut bounded = client
        .get_stream_with_max_sse_event_size(uri.clone(), None, None, None, HashMap::new(), 32)
        .await
        .unwrap();
    assert!(bounded.next().await.unwrap().is_err());
    let message = serde_json::from_value(json!({"jsonrpc":"2.0","id":1,"method":"ping"})).unwrap();
    let response = client
        .post_message_with_max_sse_event_size(uri, message, None, None, HashMap::new(), 32)
        .await
        .unwrap();
    let StreamableHttpPostResponse::Sse(mut stream, _) = response else {
        panic!("SSE response expected")
    };
    assert!(stream.next().await.unwrap().is_err());
}
