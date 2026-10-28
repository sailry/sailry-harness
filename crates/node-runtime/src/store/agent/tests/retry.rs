use super::*;
use crate::agent::canonical::{self, BlockKind};

#[tokio::test]
async fn preserves_optional_error_text() {
    let mut fixture = fixture::Fixture::new().await;
    let errors = [
        None,
        Some("First upstream error"),
        Some("Current upstream error"),
    ];
    let events = errors.into_iter().enumerate().map(|(index, error)| {
        let mut event = fixture::message("system", canonical::Role::Assistant, Vec::new());
        event.metadata.execution = Some(canonical::Execution {
            retry: Some(sailry_protocol::conversation::recovery::Retry {
                attempt: index as u64 + 1,
                fallback: false,
                limit: Some(5),
                delay_ms: None,
                transport: None,
                status: None,
                code: None,
                error: error.map(str::to_owned),
            }),
            ..Default::default()
        });
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
            let mut event = fixture::message("system", canonical::Role::Assistant, Vec::new());
            event.metadata.execution = Some(canonical::Execution {
                retry: Some(notice.clone()),
                ..Default::default()
            });
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
    let mut telemetry = fixture::message("system", canonical::Role::Assistant, Vec::new());
    telemetry.metadata.execution = Some(canonical::Execution {
        observation: Some(sailry_protocol::conversation::observation::Request {
            id: "visibility".into(),
            turn: TurnId::new(),
            turn_index: None,
            model: "fixture".into(),
            context_limit: None,
            timestamp_ms: 0,
            boundary: None,
            attempts: Vec::new(),
        }),
        ..Default::default()
    });
    telemetry.metadata.runtime = Some(canonical::Runtime {
        user_visible: false,
        ..Default::default()
    });
    let visible = async |record: &canonical::Record| -> bool {
        fixture
            .database
            .connection
            .prepare(&format!(
                "SELECT {} FROM (SELECT ?1 AS body) e CROSS JOIN (SELECT '' AS session) h",
                history::VISIBLE_EVENT
            ))
            .await
            .unwrap()
            .query_row([serde_json::to_string(record).unwrap()])
            .await
            .and_then(|row| row.get(0))
            .unwrap()
    };
    assert!(!visible(&telemetry).await);
    let mut message = telemetry.clone();
    message.metadata.runtime = None;
    message
        .message_mut()
        .unwrap()
        .content
        .push(canonical::Block {
            index: 0,
            kind: BlockKind::Text {
                text: "Visible response".into(),
            },
        });
    message.message_mut().unwrap().part_count = 1;
    assert!(visible(&message).await);
    let mut usage = telemetry.clone();
    usage.metadata.runtime = None;
    usage.message_mut().unwrap().usage = Some(canonical::Usage {
        input: Some(1),
        output: Some(0),
        total: Some(1),
        ..Default::default()
    });
    assert!(visible(&usage).await);
    let mut failure = telemetry.clone();
    failure.metadata.runtime = None;
    failure.message_mut().unwrap().error = Some(canonical::ResponseError {
        code: None,
        message: "failure".into(),
    });
    assert!(visible(&failure).await);
    let mut retry = telemetry.clone();
    retry.metadata.runtime = None;
    retry.metadata.execution.as_mut().unwrap().retry =
        Some(sailry_protocol::conversation::recovery::Retry {
            attempt: 1,
            fallback: false,
            limit: Some(5),
            delay_ms: None,
            transport: None,
            status: None,
            code: None,
            error: None,
        });
    assert!(visible(&retry).await);
}
