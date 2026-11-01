use super::*;
use sailry_protocol::{conversation::ModelApi, network::Selection};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[allow(
    dead_code,
    reason = "Shared HTTP fixture also supports provider-specific responses"
)]
#[path = "../../../../tests/discovery_support/mod.rs"]
mod http;

// Test source data only; production presets always come from a successful fetch.
const SOURCE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/catalogs/network-presets.json"
));

fn document() -> Value {
    serde_json::from_slice(SOURCE).unwrap()
}

#[test]
fn preserves_original_services() {
    let catalog = parse(SOURCE).unwrap();
    assert_eq!(catalog.version, 1);
    assert_eq!(catalog.presets.len(), 3);
    assert_eq!(catalog.presets[0].id, "global");
    assert_eq!(catalog.presets[0].selection, Selection::Iroh);
    assert_eq!(catalog.presets[0].label.label("zh-CN"), "全球");
    for (preset, id, url, label) in [
        (
            &catalog.presets[1],
            "china-tencent",
            "https://iroh-service.dux.plus",
            "中国（腾讯云）",
        ),
        (
            &catalog.presets[2],
            "china-aliyun",
            "https://aliyun-1.iroh.jujiang.me",
            "中国（阿里云）",
        ),
    ] {
        assert_eq!(preset.id, id);
        assert_eq!(
            preset.selection,
            Selection::Custom {
                relays: vec![url.into()]
            }
        );
        assert_eq!(preset.label.label("zh-CN"), label);
    }
}

#[test]
fn rejects_invalid_documents() {
    let original = document();
    let mut cases = vec![];
    for (key, value) in [("version", json!(2)), ("unknown", json!(true))] {
        let mut invalid = original.clone();
        invalid[key] = value;
        cases.push(invalid);
    }
    for (key, value) in [
        ("id", json!("")),
        ("id", json!("has spaces")),
        ("id", json!("a".repeat(65))),
        ("label", json!({"label":""})),
        ("selection", json!({"kind":"custom","relays":[]})),
        (
            "selection",
            json!({"kind":"custom","relays":["http://relay.example"]}),
        ),
        (
            "selection",
            json!({"kind":"custom","relays":["https://user:secret@relay.example"]}),
        ),
        (
            "selection",
            json!({"kind":"custom","relays":["https://relay.example?key=secret"]}),
        ),
        ("unknown", json!(true)),
    ] {
        let mut invalid = original.clone();
        invalid["presets"][0][key] = value;
        cases.push(invalid);
    }
    let mut duplicate = original.clone();
    duplicate["presets"][1]["id"] = duplicate["presets"][0]["id"].clone();
    cases.push(duplicate);
    let mut excess = original;
    excess["presets"] = json!(vec![excess["presets"][0].clone(); PRESET_LIMIT + 1]);
    cases.push(excess);
    for invalid in cases {
        assert_eq!(
            parse(&serde_json::to_vec(&invalid).unwrap())
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    assert!(parse(&vec![b' '; BYTE_LIMIT + 1]).is_err());
}

#[tokio::test]
async fn caches_only_successful_fetches() {
    let fail = Arc::new(AtomicBool::new(true));
    let failed = fail.clone();
    let server = http::Server::start(ModelApi::Responses, move |_| {
        if failed.load(Ordering::SeqCst) {
            http::Reply::Raw("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into())
        } else {
            http::Reply::Json(document())
        }
    }).await;
    let directory = Directory::default();
    directory.fixture(&server.endpoint).unwrap();
    let stop = CancellationToken::new();
    assert_eq!(
        directory.read(false, &stop).await.unwrap(),
        Listing::default()
    );
    assert!(server.requests.lock().unwrap().is_empty());
    let missing = directory.read(true, &stop).await.unwrap();
    assert!(missing.unavailable && missing.presets.is_empty());
    assert_eq!(directory.read(false, &stop).await.unwrap(), missing);
    fail.store(false, Ordering::SeqCst);
    let loaded = directory.read(true, &stop).await.unwrap();
    assert!(!loaded.unavailable);
    assert_eq!(loaded.presets, parse(SOURCE).unwrap().presets);
    assert_eq!(directory.read(false, &stop).await.unwrap(), loaded);
    directory.read(true, &stop).await.unwrap();
    assert_eq!(
        server.requests.lock().unwrap().len(),
        3,
        "each new explicit refresh fetches the source"
    );
    fail.store(true, Ordering::SeqCst);
    let stale = directory.read(true, &stop).await.unwrap();
    assert!(stale.unavailable);
    assert_eq!(stale.presets, loaded.presets);
    assert_eq!(directory.read(false, &stop).await.unwrap(), stale);
    assert_eq!(server.requests.lock().unwrap().len(), 4);
    for request in server.requests.lock().unwrap().iter() {
        assert_eq!(request.path, "/v1");
        assert_eq!(request.headers["accept"], "application/json");
        assert!(!request.headers.contains_key("authorization"));
        assert!(!request.headers.contains_key("cookie"));
    }
}

#[tokio::test]
async fn shares_overlapping_refreshes() {
    let release = Arc::new(tokio::sync::Notify::new());
    let notified = release.clone();
    let server = http::Server::start(ModelApi::Responses, move |_| {
        http::Reply::Delayed(notified.clone(), document())
    })
    .await;
    let directory = Directory::default();
    directory.fixture(&server.endpoint).unwrap();
    let stop = CancellationToken::new();
    let (first, second, ()) = tokio::join!(
        directory.read(true, &stop),
        directory.read(true, &stop),
        async {
            server.wait_requests(1).await;
            assert_eq!(
                directory.read(false, &stop).await.unwrap(),
                Listing::default()
            );
            release.notify_one();
        },
    );
    assert_eq!(first.unwrap(), second.unwrap());
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cancels_active_refreshes() {
    let server = http::Server::start(ModelApi::Responses, |_| http::Reply::Hold).await;
    let directory = Directory::default();
    directory.fixture(&server.endpoint).unwrap();
    let stop = CancellationToken::new();
    let (result, ()) = tokio::join!(directory.read(true, &stop), async {
        server.wait_requests(1).await;
        stop.cancel();
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    server.wait_closed(1).await;
    assert_eq!(
        directory
            .read(false, &CancellationToken::new())
            .await
            .unwrap(),
        Listing::default()
    );
    let fresh = http::Server::start(ModelApi::Responses, |_| http::Reply::Json(document())).await;
    directory.fixture(&fresh.endpoint).unwrap();
    assert!(
        !directory
            .read(true, &CancellationToken::new())
            .await
            .unwrap()
            .unavailable
    );
}

#[test]
fn limits_fixture_origins() {
    let directory = Directory::default();
    for endpoint in ["http://127.0.0.1:123/catalog", "http://[::1]:123/catalog"] {
        directory.fixture(endpoint).unwrap();
    }
    for endpoint in [
        "https://127.0.0.1/catalog",
        "http://localhost/catalog",
        "http://example.com/catalog",
        "http://user:secret@127.0.0.1/catalog",
        "http://127.0.0.1/catalog?key=value",
    ] {
        assert_eq!(
            directory.fixture(endpoint).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}
