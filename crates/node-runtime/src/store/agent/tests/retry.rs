use super::*;

#[test]
fn preserves_optional_error_text() {
    let mut fixture = fixture::Fixture::new();
    let errors = [
        None,
        Some("First upstream error"),
        Some("Current upstream error"),
    ];
    let events = errors.into_iter().enumerate().map(|(index, error)| {
        let mut event = AdkEvent::new("retry-fixture");
        event.author = "system".into();
        let mut value = serde_json::json!({"attempt":index + 1,"limit":5});
        if let Some(error) = error {
            value["error"] = error.into();
        }
        event
            .provider_metadata
            .insert("sailry_retry".into(), value.to_string());
        event
    });
    fixture.append(events);
    let history = fixture.read(None, 100);
    let retries: Vec<_> = history
        .page
        .entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::Resource(value) if value["type"] == "model_retry" => Some(value),
            _ => None,
        })
        .collect();
    assert_eq!(retries.len(), errors.len());
    for (index, (retry, error)) in retries.iter().zip(errors).enumerate() {
        assert_eq!(retry["attempt"], index + 1);
        assert_eq!(retry["limit"], 5);
        assert_eq!(retry["error"].as_str(), error);
    }
    assert_eq!(history.page.entries, fixture.read(None, 100).page.entries);
}

#[test]
fn retains_persistent_and_fallback_records() {
    use sailry_protocol::conversation::recovery::{Retry, Transport};
    let mut fixture = fixture::Fixture::new();
    let notices = [
        Retry {
            attempt: 8,
            fallback: false,
            limit: None,
            delay_ms: Some(60_000),
            transport: Some(Transport::Http),
            status: None,
            code: None,
            error: Some("connection failed".into()),
        },
        Retry {
            attempt: 0,
            fallback: true,
            limit: Some(5),
            delay_ms: Some(0),
            transport: Some(Transport::WebSocket),
            status: None,
            code: Some("network".into()),
            error: Some("socket closed".into()),
        },
    ];
    fixture.append(notices.iter().map(|notice| {
        let mut event = AdkEvent::new("retry-fixture");
        event.author = "system".into();
        event.provider_metadata.insert(
            "sailry_retry".into(),
            serde_json::to_string(notice).unwrap(),
        );
        event
    }));
    let page = fixture.read(None, 100).page;
    let values: Vec<_> = page
        .entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::Resource(value) => Retry::from_resource(value),
            _ => None,
        })
        .collect();
    assert_eq!(values, notices);
    assert_eq!(page.entries, fixture.read(None, 100).page.entries);
}
