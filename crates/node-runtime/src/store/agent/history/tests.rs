use super::*;
use crate::store::{
    agent::tests::fixture::{Fixture, message, record, request, response},
    database::{conflict, counters},
};
use records::Role;
use tokio::sync::{mpsc, oneshot};

mod defaults;
mod links;
mod visibility;

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

#[tokio::test]
async fn retains_called_and_discovered_tools() {
    let mut fixture = Fixture::new().await;
    let invoked = message(
        "assistant",
        Role::Assistant,
        ["first", "second"]
            .map(|name| BlockKind::ToolRequest(request(Some(name), name, serde_json::json!({}))))
            .to_vec(),
    );
    let discovered = record(
        "system",
        Kind::Configuration(records::Configuration {
            context: records::Context {
                loaded_tools: Some(["third".into()].into()),
                ..Default::default()
            },
        }),
    );
    fixture.append([invoked, discovered]).await;
    let expected = serde_json::json!(["first", "second", "third"]);
    assert_eq!(
        state(&fixture.database.connection, fixture.session)
            .await
            .unwrap()[LOADED_TOOLS],
        expected
    );
    rebuild_session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(
        state(&fixture.database.connection, fixture.session)
            .await
            .unwrap()[LOADED_TOOLS],
        expected
    );
}

fn captured(turn: TurnId) -> Record {
    let mut event = message(
        "tool",
        Role::User,
        vec![BlockKind::ToolResponse(response(
            Some("completed-write"),
            "write_file",
            serde_json::json!({"path":"saved.txt","content":"Captured tool result"}),
        ))],
    );
    event.turn = turn;
    event.metadata.state = Some(records::StateChanges {
        session: [("saved".into(), serde_json::json!(1))].into(),
        ..Default::default()
    });
    event
}

#[tokio::test]
async fn rolls_back_context_with_rejected_input() {
    let mut fixture = Fixture::new().await;
    let owner = running(&mut fixture).await;
    let mut context = message("system", Role::Assistant, Vec::new());
    context.turn = owner;
    context.metadata.state = Some(records::StateChanges {
        session: [("context_saved".into(), serde_json::json!(true))].into(),
        ..Default::default()
    });
    let mut input = message(
        "user",
        Role::User,
        vec![BlockKind::Text {
            text: "Uncommitted input".into(),
        }],
    );
    input.turn = owner;
    input.id.clear();
    let session = fixture.session;
    assert!(
        append_with_context(
            &mut fixture.database,
            session,
            owner,
            input,
            Some((owner, context)),
            &fixture.events,
        )
        .await
        .is_err()
    );
    assert!(
        !state(&fixture.database.connection, session)
            .await
            .unwrap()
            .contains_key("context_saved")
    );
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
}

#[tokio::test]
async fn preserves_explicit_visibility() {
    let fixture = Fixture::new().await;
    let mut telemetry = message("system", Role::Assistant, Vec::new());
    telemetry.metadata.runtime = Some(records::Runtime {
        user_visible: false,
        ..Default::default()
    });
    let mut visible = telemetry.clone();
    visible.metadata.runtime.as_mut().unwrap().user_visible = true;
    let mut changed = visible.clone();
    changed.metadata.context = Some(records::Context {
        policy: Some("Captured policy".into()),
        ..Default::default()
    });
    let defaulted = message("assistant", Role::Assistant, Vec::new());
    let mut query = fixture
        .database
        .connection
        .prepare(&format!(
            "SELECT {VISIBLE_EVENT} FROM (SELECT ?1 AS body) e CROSS JOIN (SELECT '' AS session) h"
        ))
        .await
        .unwrap();
    for (record, expected) in [
        (telemetry, false),
        (visible, true),
        (defaulted, true),
        (changed, true),
    ] {
        let body = encode(&record).unwrap();
        let visible = query
            .query_row([body.as_slice()])
            .await
            .unwrap()
            .get::<bool>(0)
            .unwrap();
        assert_eq!(visible, expected, "{:?}", record.metadata);
    }
}

#[tokio::test]
async fn hides_canonical_runtime_context() {
    let fixture = Fixture::new().await;
    let mut event = message(
        "system",
        Role::User,
        vec![BlockKind::Text {
            text: "Captured runtime policy".into(),
        }],
    );
    event.metadata.context = Some(records::Context {
        captured: true,
        policy: Some("Captured runtime policy".into()),
        ..Default::default()
    });
    let original = event.clone();
    let entry = project(&fixture.database.connection, 1, event.turn, &event)
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
    let event = message("", Role::Assistant, vec![
        BlockKind::ServerCall { value: serde_json::json!({
            "type":"message","phase":"commentary",
            "content":[{"type":"output_text","text":"Reading "},{"type":"output_text","text":"the source"}]
        }).into() },
        BlockKind::ToolRequest(request(Some("call"), "read_file", serde_json::json!({"path":"source.rs"}))),
    ]);
    let entry = project(&fixture.database.connection, 1, event.turn, &event)
        .await
        .unwrap();
    assert_eq!(entry.parts.len(), 2);
    assert_eq!(entry.parts[0], Part::Text("Reading the source".into()));
    assert_eq!(entry.commentary, [(0, event.id.clone())].into());
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
async fn retains_interrupted_commentary_phase() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let session = fixture.session;
    fixture
        .database
        .feeds
        .delta(
            fixture.database.node,
            session,
            Draft {
                id: "progress".into(),
                turn,
                author: "assistant".into(),
                branch: String::new(),
                parts: vec![Part::Text("Checking the file".into())],
                commentary: [(0, "msg-progress".into())].into(),
            },
        )
        .unwrap();
    let run = runs::get(&fixture.database.connection, turn).await.unwrap();
    retain(&mut fixture.database, &run, &fixture.events)
        .await
        .unwrap();
    let body: Vec<u8> = fixture
        .database
        .connection
        .prepare("SELECT body FROM agent_events WHERE session=?1 AND id='progress'")
        .await
        .unwrap()
        .query_row([session.to_string()])
        .await
        .unwrap()
        .get(0)
        .unwrap();
    let event: Record = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        event.message().unwrap().finish,
        Some(records::Finish::Interrupted)
    );
    let BlockKind::ServerCall { value } = &event.message().unwrap().content[0].kind else {
        panic!("commentary message expected")
    };
    let server_tool_call = &value.value;
    assert_eq!(server_tool_call["phase"], "commentary");
    assert_eq!(server_tool_call["id"], "msg-progress");
    assert_eq!(server_tool_call["status"], "incomplete");
    assert_eq!(
        commentary(server_tool_call).as_deref(),
        Some("Checking the file")
    );
    let entry = project(&fixture.database.connection, 1, turn, &event)
        .await
        .unwrap();
    assert_eq!(entry.commentary, [(0, "msg-progress".into())].into());
    assert_eq!(entry.parts, vec![Part::Text("Checking the file".into())]);
}

#[tokio::test]
async fn omits_interrupted_nontext_parts() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let session = fixture.session;
    fixture
        .database
        .feeds
        .delta(
            fixture.database.node,
            session,
            Draft {
                id: "interrupted-parts".into(),
                turn,
                author: "assistant".into(),
                branch: String::new(),
                parts: vec![
                    Part::Resource(serde_json::json!({"type":"transient"})),
                    Part::Thinking("Checking".into()),
                    Part::Text("Reading the source".into()),
                    Part::Resource(serde_json::json!({"type":"transient"})),
                ],
                commentary: [(2, "progress-message".into())].into(),
            },
        )
        .unwrap();
    let run = runs::get(&fixture.database.connection, turn).await.unwrap();
    retain(&mut fixture.database, &run, &fixture.events)
        .await
        .unwrap();
    let history = fixture.read(None, 100).await;
    let entry = history
        .page
        .entries
        .iter()
        .find(|entry| entry.id == "interrupted-parts")
        .unwrap();
    assert_eq!(
        entry.parts,
        vec![
            Part::Thinking("Checking".into()),
            Part::Text("Reading the source".into())
        ]
    );
    assert_eq!(entry.commentary, [(1, "progress-message".into())].into());
}

#[tokio::test]
async fn publishes_only_settled_batches() {
    let mut fixture = Fixture::new().await;
    let turn = running(&mut fixture).await;
    let session = fixture.session;
    let mut changes = fixture.events.subscribe();
    let call = |id: &str| {
        BlockKind::ToolRequest(request(
            Some(id),
            "read_file",
            serde_json::json!({"path":"source.rs"}),
        ))
    };
    let result = |id: &str| {
        BlockKind::ToolResponse(response(
            Some(id),
            "read_file",
            serde_json::json!({"text":"source"}),
        ))
    };
    let event = |id: &str, role, parts| {
        let mut event = message("assistant", role, parts);
        event.id = id.into();
        event.turn = turn;
        event
    };
    let mut published = Vec::new();
    let second = event("second", Role::User, vec![result("second")]);
    for (value, expected) in [
        (
            event(
                "calls",
                Role::Assistant,
                vec![call("first"), call("second")],
            ),
            0,
        ),
        (second.clone(), 0),
        (second, 0),
        (event("first", Role::User, vec![result("first")]), 1),
        (event("unmatched", Role::User, vec![result("other")]), 1),
        (
            event(
                "answer",
                Role::Assistant,
                vec![BlockKind::Text {
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
    let event = captured(turn);
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
    let stored: Record = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        serde_json::to_value(&stored.message().unwrap().content).unwrap(),
        serde_json::to_value(&event.message().unwrap().content).unwrap()
    );
    assert!(matches!(
        stored.message().unwrap().role,
        records::Role::User
    ));
    assert_eq!(
        stored.metadata.state.as_ref().unwrap().session["saved"],
        serde_json::json!(1)
    );
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
    let event = captured(turn);
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
        error.code == ErrorCode::Busy && error.message.contains("storage transaction conflicted"),
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

#[tokio::test]
async fn retains_slots_around_native_search() {
    let fixture = Fixture::new().await;
    let turn = TurnId::new();
    let mut signed = request(
        Some("read"),
        "read_file",
        serde_json::json!({"path":"source.rs"}),
    );
    signed.signature = Some("signature".into());
    let mut record = message("assistant", Role::Assistant, vec![
        BlockKind::ServerCall { value: serde_json::json!({"type":"web_search_call","id":"search","status":"completed","action":{"query":"source"}}).into() },
        BlockKind::Text { text: "Engine slot".into() },
        BlockKind::ToolRequest(signed),
    ]);
    record.turn = turn;
    record.provenance.branch = "search-branch".into();
    let db = &fixture.database.connection;
    record.message_mut().unwrap().content.remove(1);
    let entry = project(db, 1, turn, &record).await.unwrap();
    assert_eq!(entry.author, "assistant");
    assert_eq!(entry.branch, "search-branch");
    assert_eq!(entry.timestamp_ms, record.created.timestamp_millis());
    assert_eq!(entry.parts.len(), 4);
    assert_eq!(
        entry.parts[1],
        Part::Resource(serde_json::json!({"type":"provider_context"}))
    );
    assert!(matches!(&entry.parts[2], Part::ToolCall { id:Some(id), .. } if id == "read"));
    assert!(matches!(&entry.parts[3], Part::ToolResult { id:Some(id), .. } if id == "search"));
    let BlockKind::ToolRequest(request) = &record.message().unwrap().content[1].kind else {
        panic!("tool request expected")
    };
    assert_eq!(request.signature.as_deref(), Some("signature"));
    assert_eq!(record.message().unwrap().content[1].index, 2);
    assert_eq!(
        tools::raw(db, turn, 1, &record, 2)
            .await
            .unwrap()
            .unwrap()
            .id
            .as_deref(),
        Some("read")
    );
    assert_eq!(
        tools::raw(db, turn, 1, &record, 3)
            .await
            .unwrap()
            .unwrap()
            .id
            .as_deref(),
        Some("search")
    );
}
