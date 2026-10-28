use super::*;

#[tokio::test]
async fn preserves_optional_error_text() {
    let mut fixture = fixture::Fixture::new().await;
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
    fixture.append(events).await;
    let history = fixture.read(None, 100).await;
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
    assert_eq!(
        history.page.entries,
        fixture.read(None, 100).await.page.entries
    );
}

#[tokio::test]
async fn retains_persistent_and_fallback_records() {
    use sailry_protocol::conversation::recovery::{Retry, Transport};
    let mut fixture = fixture::Fixture::new().await;
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
    fixture
        .append(notices.iter().map(|notice| {
            let mut event = AdkEvent::new("retry-fixture");
            event.author = "system".into();
            event.provider_metadata.insert(
                "sailry_retry".into(),
                serde_json::to_string(notice).unwrap(),
            );
            event
        }))
        .await;
    let page = fixture.read(None, 100).await.page;
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
    assert_eq!(page.entries, fixture.read(None, 100).await.page.entries);
}

#[tokio::test]
async fn telemetry_preserves_visible_events() {
    let fixture = fixture::Fixture::new().await;
    let mut event = AdkEvent::new("telemetry-fixture");
    event.author = "system".into();
    event.provider_metadata.insert(
        "sailry_model_observation".into(),
        serde_json::json!({
            "id":"visibility", "turn":TurnId::new(), "model":"fixture",
            "context_limit":null, "timestamp_ms":0, "boundary":null, "attempts":[]
        })
        .to_string(),
    );
    let pure = serde_json::to_value(event).unwrap();
    let visible = async |body: &serde_json::Value| -> bool {
        let event = serde_json::from_value(body.clone()).unwrap();
        let record = crate::agent::canonical_bridge::from_adk(
            &fixture.database.connection,
            TurnId::new(),
            &event,
        )
        .await
        .unwrap();
        fixture
            .database
            .connection
            .prepare(&format!(
                "SELECT {} FROM (SELECT ?1 AS body) e",
                history::VISIBLE_EVENT
            ))
            .await
            .unwrap()
            .query_row([serde_json::to_string(&record).unwrap()])
            .await
            .and_then(|row| row.get(0))
            .unwrap()
    };
    assert!(!visible(&pure).await);
    for (field, value) in [
        ("state_delta", serde_json::json!({"state":true})),
        ("artifact_delta", serde_json::json!({"file":1})),
        (
            "tool_confirmation",
            serde_json::to_value(crate::agent::canonical::ToolIdentity::new(
                "read_file",
                serde_json::json!({}),
            ))
            .unwrap(),
        ),
        (
            "tool_confirmation_decision",
            serde_json::to_value(adk_core::ToolConfirmationDecision::Approve).unwrap(),
        ),
        ("transfer_to_agent", serde_json::json!("child")),
        ("escalate", serde_json::json!(true)),
        ("route", serde_json::json!(["child"])),
        (
            "workflow_positions",
            serde_json::json!({"step":{"iteration":1,"sub_agent":"child"}}),
        ),
    ] {
        let mut mixed = pure.clone();
        mixed["actions"][field] = value;
        assert!(visible(&mixed).await, "action {field} remains visible");
    }
    for (field, value) in [
        (
            "usage_metadata",
            serde_json::json!({"prompt_token_count":1,"candidates_token_count":0,"total_token_count":1}),
        ),
        ("error_message", serde_json::json!("failure")),
        ("content", serde_json::json!({"role":"model","parts":[]})),
    ] {
        let mut mixed = pure.clone();
        mixed[field] = value;
        assert!(visible(&mixed).await, "response {field} remains visible");
    }
    let mut retry = pure.clone();
    retry["event_metadata"]["sailry_retry"] = r#"{"attempt":1,"limit":5}"#.into();
    assert!(visible(&retry).await);
    let mut extension = pure.clone();
    extension["event_metadata"]["extension"] = "unknown metadata".into();
    assert!(visible(&extension).await);
    let mut missing = pure;
    missing["actions"]
        .as_object_mut()
        .unwrap()
        .remove("state_delta");
    assert!(serde_json::from_value::<AdkEvent>(missing).is_err());
}
