use super::*;
use reqwest::header::{AUTHORIZATION, HeaderMap};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[test]
fn adds_http_defaults() {
    let mut body = json!({"stream":true,"input":["fixture"]});
    let before = body.clone();
    let mut headers =
        HeaderMap::from_iter([(AUTHORIZATION, HeaderValue::from_static("Bearer fixture"))]);
    adapter(None, None)(&mut body, &mut headers).unwrap();
    assert_eq!(headers[ACCEPT], "text/event-stream");
    assert_eq!(headers[CACHE_CONTROL], "no-cache");
    assert_eq!(headers[AUTHORIZATION], "Bearer fixture");
    assert_eq!(body, before);
}

#[test]
fn preserves_other_modes() {
    for (stream, websocket) in [(false, false), (false, true), (true, true)] {
        let options = websocket.then(WsOptions::default);
        let mut body = json!({"stream":stream,"input":["fixture"]});
        let before = body.clone();
        let mut headers =
            HeaderMap::from_iter([(AUTHORIZATION, HeaderValue::from_static("Bearer fixture"))]);
        let expected = headers.clone();
        adapter(None, options.as_ref())(&mut body, &mut headers).unwrap();
        assert_eq!(headers, expected);
        assert_eq!(body, before);
    }
}

#[test]
fn uses_http_for_background() {
    let mut body = json!({"stream":true,"background":true});
    let mut headers = HeaderMap::new();
    adapter(None, Some(&WsOptions::default()))(&mut body, &mut headers).unwrap();
    assert_eq!(headers[ACCEPT], "text/event-stream");
    assert_eq!(headers[CACHE_CONTROL], "no-cache");
}

#[test]
fn preserves_explicit_headers() {
    let mut headers = HeaderMap::from_iter([
        (
            ACCEPT,
            HeaderValue::from_static("application/vnd.fixture+json"),
        ),
        (CACHE_CONTROL, HeaderValue::from_static("no-store")),
    ]);
    let expected = headers.clone();
    adapter(None, None)(&mut json!({"stream":true}), &mut headers).unwrap();
    assert_eq!(headers, expected);
}

#[test]
fn keeps_provider_precedence() {
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let provider: RequestAdapter = Arc::new(move |_, headers| {
        called.fetch_add(1, Ordering::Relaxed);
        headers.remove(AUTHORIZATION);
        headers.insert("x-provider-key", HeaderValue::from_static("fixture"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.fixture+json"),
        );
        headers.remove(CACHE_CONTROL);
        Ok(())
    });
    let mut headers =
        HeaderMap::from_iter([(AUTHORIZATION, HeaderValue::from_static("Bearer fixture"))]);
    adapter(Some(provider), None)(&mut json!({"stream":true}), &mut headers).unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(!headers.contains_key(AUTHORIZATION));
    assert_eq!(headers["x-provider-key"], "fixture");
    assert_eq!(headers[ACCEPT], "application/vnd.fixture+json");
    assert!(!headers.contains_key(CACHE_CONTROL));
}

#[test]
fn follows_existing_fallback_policy() {
    let enabled = Arc::new(AtomicBool::new(true));
    let policy = enabled.clone();
    let options = WsOptions {
        enabled: Arc::new(move || policy.load(Ordering::Relaxed)),
        ..Default::default()
    };
    let adapt = adapter(None, Some(&options));
    let mut headers = HeaderMap::new();
    adapt(&mut json!({"stream":true}), &mut headers).unwrap();
    assert!(headers.is_empty());
    enabled.store(false, Ordering::Relaxed);
    adapt(&mut json!({"stream":true}), &mut headers).unwrap();
    assert_eq!(headers[ACCEPT], "text/event-stream");
    assert_eq!(headers[CACHE_CONTROL], "no-cache");
}
