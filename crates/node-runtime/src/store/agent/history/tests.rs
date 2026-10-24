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
