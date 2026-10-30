use super::*;
use sailry_protocol::search::{Driver, Freshness, Result};

fn input(query: &str) -> Request {
    Request {
        query: query.into(),
        ..Default::default()
    }
}

fn key(query: &str) -> Key {
    Key::new(Some(Selection::Auto), input(query))
}

fn page(title: &str) -> Page {
    Page {
        summary: None,
        driver: Driver::Tavily,
        results: vec![Result {
            title: title.into(),
            url: "https://example.com/source".into(),
            snippet: "Untrusted excerpt".into(),
            published: None,
        }],
        untrusted: true,
        fetch_hint: "One provider page".into(),
    }
}

#[test]
fn expires_without_extending_on_hits() {
    let mut cache = Cache::default();
    let now = Instant::now();
    cache.insert(key("source"), page("Source"), now);
    assert!(
        cache
            .get(&key("source"), now + LIFETIME - Duration::from_secs(1))
            .is_some()
    );
    assert!(cache.get(&key("source"), now + LIFETIME).is_none());
    assert!(cache.entries.is_empty());
}

#[test]
fn bounds_entries_and_keeps_recently_used() {
    let mut cache = Cache::default();
    let now = Instant::now();
    for index in 0..CAPACITY {
        cache.insert(key(&index.to_string()), page("Source"), now);
    }
    assert_eq!(cache.entries.len(), CAPACITY);
    assert!(cache.get(&key("0"), now).is_some());
    cache.insert(key("new"), page("New source"), now);
    assert_eq!(cache.entries.len(), CAPACITY);
    assert!(cache.get(&key("1"), now).is_none());
    assert!(cache.get(&key("0"), now).is_some());
    cache.insert(key("new"), page("Updated source"), now);
    assert_eq!(cache.entries.len(), CAPACITY);
    assert_eq!(
        cache.get(&key("new"), now).unwrap().results[0].title,
        "Updated source"
    );
}

#[test]
fn normalizes_requests_and_distinguishes_selection() {
    let mut cache = Cache::default();
    let now = Instant::now();
    let original = Request {
        query: " source ".into(),
        include_domains: vec![
            "DOCS.example.org".into(),
            "EXAMPLE.COM.".into(),
            "example.com".into(),
        ],
        exclude_domains: vec!["PRIVATE.example.com".into()],
        ..Default::default()
    };
    original.validate().unwrap();
    cache.insert(
        Key::new(Some(Selection::Auto), original.clone()),
        page("Source"),
        now,
    );
    let mut normalized = original.normalized();
    assert!(
        cache
            .get(&Key::new(Some(Selection::Auto), normalized.clone()), now)
            .is_some()
    );
    assert!(
        cache
            .get(
                &Key::new(Some(Selection::Driver(Driver::Tavily)), normalized.clone()),
                now
            )
            .is_none()
    );
    normalized.query = "other source".into();
    assert!(
        cache
            .get(&Key::new(Some(Selection::Auto), normalized), now)
            .is_none()
    );
}

#[test]
fn distinguishes_filters_and_limits() {
    let mut cache = Cache::default();
    let now = Instant::now();
    let request = input("source");
    cache.insert(
        Key::new(Some(Selection::Auto), request.clone()),
        page("Source"),
        now,
    );
    let changes = [
        Request {
            limit: 1,
            ..request.clone()
        },
        Request {
            freshness: Some(Freshness::Day),
            ..request.clone()
        },
        Request {
            include_domains: vec!["example.com".into()],
            ..request.clone()
        },
        Request {
            exclude_domains: vec!["private.example.com".into()],
            ..request
        },
    ];
    for request in changes {
        assert!(
            cache
                .get(&Key::new(Some(Selection::Auto), request), now)
                .is_none()
        );
    }
}

#[test]
fn clears_all_entries() {
    let mut cache = Cache::default();
    let now = Instant::now();
    cache.insert(key("source"), page("Source"), now);
    cache.clear();
    assert!(cache.get(&key("source"), now).is_none());
}
