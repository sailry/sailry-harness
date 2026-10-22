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

#[test]
fn telemetry_preserves_visible_events() {
    let fixture = fixture::Fixture::new();
    let mut event = AdkEvent::new("telemetry-fixture");
    event.author = "system".into();
    event
        .provider_metadata
        .insert("sailry_model_observation".into(), "{}".into());
    let pure = serde_json::to_value(event).unwrap();
    let visible = |body: &serde_json::Value| -> bool {
        fixture
            .database
            .connection
            .query_row(
                &format!(
                    "SELECT {} FROM (SELECT ?1 AS body) e",
                    history::VISIBLE_EVENT
                ),
                [serde_json::to_string(body).unwrap()],
                |row| row.get(0),
            )
            .unwrap()
    };
    assert!(!visible(&pure));
    for (field, value) in [
        ("state_delta", serde_json::json!({"state":true})),
        ("artifact_delta", serde_json::json!({"file":1})),
        ("tool_confirmation", serde_json::json!({"id":"tool"})),
        (
            "tool_confirmation_decision",
            serde_json::json!({"id":"tool"}),
        ),
        ("transfer_to_agent", serde_json::json!("child")),
        ("escalate", serde_json::json!(true)),
        ("route", serde_json::json!(["child"])),
        ("workflow_positions", serde_json::json!({"step":1})),
    ] {
        let mut mixed = pure.clone();
        mixed["actions"][field] = value;
        assert!(visible(&mixed), "action {field} remains visible");
    }
    for (field, value) in [
        (
            "usage_metadata",
            serde_json::json!({"prompt_token_count":1}),
        ),
        ("error_message", serde_json::json!("failure")),
        ("content", serde_json::json!({"role":"model","parts":[]})),
    ] {
        let mut mixed = pure.clone();
        mixed[field] = value;
        assert!(visible(&mixed), "response {field} remains visible");
    }
    let mut retry = pure.clone();
    retry["event_metadata"]["sailry_retry"] = "{}".into();
    assert!(visible(&retry));
    let mut missing = pure;
    missing["actions"]
        .as_object_mut()
        .unwrap()
        .remove("state_delta");
    assert!(visible(&missing));
}
