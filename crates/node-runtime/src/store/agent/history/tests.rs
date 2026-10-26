use super::*;
use crate::store::{
    agent::tests::fixture::Fixture,
    database::{conflict, counters},
};
use tokio::sync::{mpsc, oneshot};

tokio::task_local! {
    static WRITES: mpsc::UnboundedSender<oneshot::Sender<()>>;
}

pub(super) async fn before_write() -> Result<(), Fault> {
    let Ok(sender) = WRITES.try_with(Clone::clone) else {
        return Ok(());
    };
    let (reply, released) = oneshot::channel();
    if sender.send(reply).is_ok() {
        released.await.map_err(storage_error)?;
    }
    Ok(())
}

async fn running(fixture: &mut Fixture) -> TurnId {
    let turn = fixture.queued().await;
    super::super::runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    assert_eq!(
        super::super::runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap()
            .turn
            .id,
        turn
    );
    turn
}

fn captured() -> AdkEvent {
    let mut event = AdkEvent::new("captured-write");
    event.author = "tool".into();
    event.set_content(adk_core::Content {
        role: "user".into(),
        parts: vec![adk_core::Part::FunctionResponse {
            id: Some("completed-write".into()),
            function_response: adk_core::FunctionResponseData::new(
                "write_file",
                serde_json::json!({"path":"saved.txt","content":"Captured tool result"}),
            ),
            annotations: None,
        }],
    });
    event
        .actions
        .state_delta
        .insert("saved".into(), serde_json::json!(1));
    event
}

#[tokio::test]
async fn hides_canonical_runtime_context() {
    let fixture = Fixture::new().await;
    let mut event = AdkEvent::new("context");
    event.author = "system".into();
    event
        .provider_metadata
        .insert(crate::agent::CONTEXT_METADATA.into(), "context".into());
    event.set_content(adk_core::Content::new("user").with_text("Captured runtime policy"));
    let original = event.clone();
    let entry = project(&fixture.database.connection, 1, TurnId::new(), &event)
        .await
        .unwrap();
    assert!(entry.parts.is_empty());
    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::to_value(original).unwrap()
    );
}

#[tokio::test]
async fn preserves_commentary_part_indices() {
    let fixture = Fixture::new().await;
    let mut event = AdkEvent::new("commentary");
    event.set_content(adk_core::Content {
        role:"model".into(),
        parts:vec![
            adk_core::Part::ServerToolCall {server_tool_call:serde_json::json!({
                "type":"message","phase":"commentary",
                "content":[{"type":"output_text","text":"Reading "},{"type":"output_text","text":"the source"}]
            })},
            adk_core::Part::FunctionCall {id:Some("call".into()),name:"read_file".into(),
                args:serde_json::json!({"path":"source.rs"}),thought_signature:None},
        ],
    });
    let entry = project(&fixture.database.connection, 1, TurnId::new(), &event)
        .await
        .unwrap();
    assert_eq!(entry.parts.len(), 2);
    assert_eq!(entry.parts[0], Part::Text("Reading the source".into()));
    assert!(matches!(&entry.parts[1],Part::ToolCall {id:Some(id),..} if id == "call"));
}

#[test]
fn projects_commentary_deltas() {
    let value = serde_json::json!({
        "type":"message","phase":"commentary","content":[{"type":"output_text","text":"Inspecting"}]
    });
    assert_eq!(
        commentary(&value).map(Part::Text),
        Some(Part::Text("Inspecting".into()))
    );
    assert!(commentary(&serde_json::json!({
        "type":"message","phase":"final_answer","content":[{"type":"output_text","text":"Answer"}]
    })).is_none());
}

#[tokio::test]
async fn publishes_only_settled_batches() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let session = fixture.session;
    let mut changes = fixture.events.subscribe();
    let call = |id: &str| adk_core::Part::FunctionCall {
        name: "read_file".into(),
        args: serde_json::json!({"path":"source.rs"}),
        id: Some(id.into()),
        thought_signature: None,
    };
    let result = |id: &str| adk_core::Part::FunctionResponse {
        function_response: adk_core::FunctionResponseData::new(
            "read_file",
            serde_json::json!({"text":"source"}),
        ),
        id: Some(id.into()),
        annotations: None,
    };
    let event = |id: &str, role: &str, parts| {
        let mut event = AdkEvent::new("batch");
        event.id = id.into();
        event.author = "assistant".into();
        event.set_content(adk_core::Content {
            role: role.into(),
            parts,
        });
        event
    };
    let mut published = Vec::new();
    for (value, expected) in [
        (
            event("calls", "model", vec![call("first"), call("second")]),
            0,
        ),
        (event("second", "function", vec![result("second")]), 0),
        (event("second", "function", vec![result("second")]), 0),
        (event("first", "function", vec![result("first")]), 1),
        (event("unmatched", "function", vec![result("other")]), 1),
        (
            event(
                "answer",
                "model",
                vec![adk_core::Part::Text {
                    text: "Done".into(),
                }],
            ),
            1,
        ),
    ] {
        append(&mut fixture.database, session, turn, value, &fixture.events)
            .await
            .unwrap();
        while let Ok(value) = changes.try_recv() {
            if let Event::ToolsCompleted { session: owner } = value.event {
                assert_eq!(owner, session);
                published.push(value.cursor);
            }
        }
        assert_eq!(published.len(), expected);
    }
}

async fn advance(connection: &Connection) -> u64 {
    let transaction = conflict::begin(connection).await.unwrap();
    let sequence = counters::next(&transaction, counters::Namespace::AgentEvents)
        .await
        .unwrap();
    conflict::commit(transaction).await.unwrap();
    sequence
}

#[tokio::test]
async fn retains_captured_results_after_conflict() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let event = captured();
    let session = fixture.session;
    let other = fixture.database.handle.lease(false).await.unwrap();
    let mut published = fixture.events.subscribe();
    let (writes, mut started) = mpsc::unbounded_channel();
    let persist = WRITES.scope(
        writes,
        append(
            &mut fixture.database,
            session,
            turn,
            event.clone(),
            &fixture.events,
        ),
    );
    let competing = async {
        let release = started.recv().await.unwrap();
        let sequence = advance(&other).await;
        drop(started);
        release.send(()).unwrap();
        sequence
    };
    let (result, sequence) = tokio::join!(persist, competing);
    result.unwrap();
    assert!(fixture.database.connection.is_autocommit().unwrap());
    let body: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT body FROM agent_events WHERE session=?1 AND id=?2")
        .await
        .unwrap()
        .query_row(params![session.to_string(), event.id.as_str()])
        .await
        .unwrap()
        .get(0)
        .unwrap();
    let stored: AdkEvent = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        serde_json::to_value(stored.content()).unwrap(),
        serde_json::to_value(event.content()).unwrap()
    );
    assert_eq!(stored.actions.state_delta, event.actions.state_delta);
    assert_eq!(
        state(&fixture.database.connection, session).await.unwrap()["saved"],
        1
    );
    append(&mut fixture.database, session, turn, event, &fixture.events)
        .await
        .unwrap();
    let count: i64 = fixture
        .database
        .connection
        .prepare("SELECT count(*) FROM agent_events WHERE session=?1")
        .await
        .unwrap()
        .query_row([session.to_string()])
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(count, 1);
    let watermark: i64 = fixture
        .database
        .connection
        .prepare("SELECT value FROM counters WHERE namespace='agent_events'")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(watermark as u64, sequence + 1);
    assert!(published.try_recv().is_ok());
    assert!(matches!(
        published.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn bounds_conflicts_without_partial_history() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let event = captured();
    let session = fixture.session;
    let other = fixture.database.handle.lease(false).await.unwrap();
    let mut published = fixture.events.subscribe();
    let (writes, mut started) = mpsc::unbounded_channel();
    let persist = WRITES.scope(
        writes,
        append(&mut fixture.database, session, turn, event, &fixture.events),
    );
    let competing = async {
        let mut sequence = 0;
        let mut attempts = 0;
        while let Some(release) = started.recv().await {
            attempts += 1;
            assert!(attempts <= conflict::RETRIES + 1);
            sequence = advance(&other).await;
            release.send(()).unwrap();
        }
        (sequence, attempts)
    };
    let (result, (sequence, attempts)) = tokio::join!(persist, competing);
    assert_eq!(
        attempts,
        conflict::RETRIES + 1,
        "history append ended after {attempts} conflicts: {result:?}"
    );
    let error = result.unwrap_err();
    assert!(
        error.message.starts_with("Busy:")
            && error.message.contains("storage transaction conflicted"),
        "{error:?}"
    );
    assert!(fixture.database.connection.is_autocommit().unwrap());
    let count: i64 = fixture
        .database
        .connection
        .prepare("SELECT count(*) FROM agent_events WHERE session=?1")
        .await
        .unwrap()
        .query_row([session.to_string()])
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(count, 0);
    assert!(
        !state(&fixture.database.connection, session)
            .await
            .unwrap()
            .contains_key("saved")
    );
    let watermark: i64 = fixture
        .database
        .connection
        .prepare("SELECT value FROM counters WHERE namespace='agent_events'")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(watermark as u64, sequence);
    assert!(matches!(
        published.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
}
