use super::*;
use sailry_protocol::conversation::partners::Protocol;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

const FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/model-partners.json");

#[test]
fn reads_public_suggestions() {
    let catalog = parse(FIXTURE).unwrap();
    let partner = &catalog.partners[0];
    assert_eq!(partner.id, "example");
    assert_eq!(partner.models, ["example-text", "example-vision"]);
    assert_eq!(
        partner
            .routes
            .iter()
            .map(|route| route.protocol)
            .collect::<Vec<_>>(),
        [
            Protocol::OpenAiResponses,
            Protocol::OpenAiChatCompletions,
            Protocol::Anthropic,
            Protocol::Gemini,
        ]
    );
    let mut external: Value = serde_json::from_slice(FIXTURE).unwrap();
    external["partners"][0]["logo"] = json!("https://example.com/logo.svg");
    assert!(parse(&serde_json::to_vec(&external).unwrap()).is_ok());
    assert_eq!(
        parse(BUNDLED).unwrap(),
        serde_json::from_slice::<Catalog>(BUNDLED).unwrap()
    );
}

#[test]
fn rejects_invalid_documents() {
    let original: Value = serde_json::from_slice(FIXTURE).unwrap();
    let mut cases = vec![];
    for (key, value) in [("version", json!(2)), ("unexpected", json!(true))] {
        let mut invalid = original.clone();
        invalid[key] = value;
        cases.push(invalid);
    }
    for (key, value) in [
        ("id", json!(" ")),
        ("name", json!("")),
        ("routes", json!([])),
        ("logo", json!("icons/providers/../../private.svg")),
        ("homepage", json!("https://user:private@example.com/")),
        ("key_url", json!("http://example.com/keys")),
        ("models", json!(["same", "same"])),
        ("models", json!([""])),
    ] {
        let mut invalid = original.clone();
        invalid["partners"][0][key] = value;
        cases.push(invalid);
    }
    let mut duplicate = original.clone();
    duplicate["partners"]
        .as_array_mut()
        .unwrap()
        .push(original["partners"][0].clone());
    cases.push(duplicate);
    let mut duplicate = original.clone();
    duplicate["partners"][0]["routes"][1] = original["partners"][0]["routes"][0].clone();
    cases.push(duplicate);
    for (key, value) in [
        ("protocol", "unknown"),
        ("endpoint", "http://example.com/v1"),
        ("endpoint", "https://example.com/v1?api_key=private"),
    ] {
        let mut invalid = original.clone();
        invalid["partners"][0]["routes"][0][key] = json!(value);
        cases.push(invalid);
    }
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
async fn shares_concurrent_loads() {
    let directory = Directory::default();
    let stop = CancellationToken::new();
    let calls = AtomicUsize::new(0);
    let load = || async {
        calls.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        parse(FIXTURE)
    };
    let (first, second, third) = tokio::join!(
        directory.cached("fixture", &stop, load()),
        directory.cached("fixture", &stop, load()),
        directory.cached("fixture", &stop, load()),
    );
    let expected = first.unwrap();
    assert!(!expected.unavailable);
    assert_eq!(second.unwrap(), expected);
    assert_eq!(third.unwrap(), expected);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retains_valid_cache_after_failure() {
    let directory = Directory::default();
    let stop = CancellationToken::new();
    let first = directory
        .cached("fixture", &stop, async { parse(FIXTURE) })
        .await
        .unwrap();
    directory.document.lock().await.as_mut().unwrap().attempted = Instant::now() - TTL;
    let fallback = directory
        .cached("fixture", &stop, async { Err(unavailable()) })
        .await
        .unwrap();
    assert!(fallback.unavailable);
    assert_eq!(fallback.catalog, first.catalog);
    let cached = directory
        .cached("fixture", &stop, async { panic!("failure must be cached") })
        .await
        .unwrap();
    assert_eq!(cached, fallback);
    directory.document.lock().await.as_mut().unwrap().attempted = Instant::now() - FAILURE_TTL;
    let restored = directory
        .cached("fixture", &stop, async { parse(BUNDLED) })
        .await
        .unwrap();
    assert!(!restored.unavailable);
    assert_eq!(restored.catalog, parse(BUNDLED).unwrap());
}

#[tokio::test]
async fn isolates_sources_and_cancellation() {
    let directory = Directory::default();
    let stop = CancellationToken::new();
    directory
        .cached("first", &stop, async { parse(FIXTURE) })
        .await
        .unwrap();
    let fallback = directory
        .cached("second", &stop, async { Err(invalid()) })
        .await
        .unwrap();
    assert!(fallback.unavailable);
    assert_eq!(fallback.catalog, parse(BUNDLED).unwrap());
    stop.cancel();
    let error = directory
        .cached("second", &stop, std::future::pending())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
}
