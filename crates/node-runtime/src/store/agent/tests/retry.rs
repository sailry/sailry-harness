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
