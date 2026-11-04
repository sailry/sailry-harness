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
async fn preserves_recorded_history() {
    let mut fixture = Fixture::new().await;
    for (index, scenario) in [
        "retains_called_and_discovered_tools",
        "rolls_back_context_with_rejected_input",
        "rejects_another_turn_record",
        "preserves_explicit_visibility",
        "hides_canonical_runtime_context",
        "omits_interrupted_nontext_parts",
        "publishes_only_settled_batches",
        "retains_captured_results_after_conflict",
        "bounds_conflicts_without_partial_history",
        "retains_slots_around_provider_context",
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            fixture.fresh_session().await;
        }
        eprintln!("scenario: {scenario}");
        match scenario {
            "retains_called_and_discovered_tools" => Box::pin(async {
                let invoked = message(
        "assistant",
                    Role::Assistant,
                    ["zeta", "alpha"]
                        .map(|name| BlockKind::ToolRequest(request(name, name, serde_json::json!({}))))
                        .to_vec(),
                );
                let discovered = record(
        "system",
                    Kind::Configuration(records::Configuration {
                        context: records::Context {
                            loaded_tools: Some(["middle".into(), "zeta".into()].into()),
                            ..Default::default()
                        },
                    }),
                );
                fixture.append([invoked, discovered]).await;
                let expected = serde_json::json!(["zeta", "alpha", "middle"]);
                assert_eq!(
                    state(&fixture.database.connection, fixture.session)
                        .await
                        .unwrap()[LOADED_TOOLS],
                    expected
                , "{scenario}");
                rebuild_session(&fixture.database.connection, fixture.session)
                    .await
                    .unwrap();
                assert_eq!(
                    state(&fixture.database.connection, fixture.session)
                        .await
                        .unwrap()[LOADED_TOOLS],
                    expected
                , "{scenario}");
            }).await,
            "rolls_back_context_with_rejected_input" => Box::pin(async {
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
                , "{scenario}");
                assert!(
                    !state(&fixture.database.connection, session)
                        .await
                        .unwrap()
                        .contains_key("context_saved")
                , "{scenario}");
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
                assert_eq!(count, 0, "{scenario}");
            }).await,
            "rejects_another_turn_record" => Box::pin(async {
                let previous = fixture.completed(&["Previous answer"]).await;
                let current = running(&mut fixture).await;
                let mut event = message("assistant", Role::Assistant, Vec::new());
                event.turn = previous;
                let error = append(
                    &mut fixture.database,
                    fixture.session,
                    current,
                    event,
                    &fixture.events,
                )
                .await
                .unwrap_err();
                assert_eq!(error.message, "event belongs to another turn", "{scenario}");
            }).await,
            "preserves_explicit_visibility" => Box::pin(async {
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
                    assert_eq!(visible, expected, "{scenario}: {:?}", record.metadata);
                }
            }).await,
            "hides_canonical_runtime_context" => Box::pin(async {
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
                assert!(entry.parts.is_empty(), "{scenario}");
                assert_eq!(
                    serde_json::to_value(event).unwrap(),
                    serde_json::to_value(original).unwrap()
                , "{scenario}");
            }).await,
            "omits_interrupted_nontext_parts" => Box::pin(async {
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
                , "{scenario}");
                let body: Vec<u8> = fixture
                    .database
                    .connection
                    .prepare("SELECT body FROM agent_events WHERE turn=?1 AND id='interrupted-parts'")
                    .await
                    .unwrap()
                    .query_row([turn.to_string()])
                    .await
                    .unwrap()
                    .get(0)
                    .unwrap();
                let record: Record = serde_json::from_slice(&body).unwrap();
                let message = record.message().unwrap();
                assert_eq!(record.turn, turn, "{scenario}");
                assert_eq!(message.finish, Some(records::Finish::Interrupted), "{scenario}");
                assert_eq!(message.part_count, 2, "{scenario}");
                assert_eq!(
                    message
                        .content
                        .iter()
                        .map(|block| block.index)
                        .collect::<Vec<_>>(),
                    [0, 1]
                , "{scenario}");
            }).await,
            "publishes_only_settled_batches" => Box::pin(async {
                let turn = running(&mut fixture).await;
                let session = fixture.session;
                let mut changes = fixture.events.subscribe();
                let call = |id: &str| {
                    BlockKind::ToolRequest(request(
                        id,
            "read_file",
                        serde_json::json!({"path":"source.rs"}),
                    ))
                };
                let result = |id: &str| {
                    BlockKind::ToolResponse(response(
                        id,
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
                            assert_eq!(owner, session, "{scenario}");
                            published.push(value.cursor);
                        }
                    }
                    assert_eq!(published.len(), expected, "{scenario}");
                }
            }).await,
            "retains_captured_results_after_conflict" => Box::pin(async {
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
                assert!(fixture.database.connection.is_autocommit().unwrap(), "{scenario}");
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
                , "{scenario}");
                assert!(matches!(
                    stored.message().unwrap().role,
                    records::Role::User
                ), "{scenario}");
                assert_eq!(
                    stored.metadata.state.as_ref().unwrap().session["saved"],
                    serde_json::json!(1)
                , "{scenario}");
                assert_eq!(
                    state(&fixture.database.connection, session).await.unwrap()["saved"],
                    1
                , "{scenario}");
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
                assert_eq!(count, 1, "{scenario}");
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
                assert_eq!(watermark as u64, sequence + 1, "{scenario}");
                assert!(published.try_recv().is_ok(), "{scenario}");
                assert!(matches!(
                    published.try_recv(),
                    Err(broadcast::error::TryRecvError::Empty)
                ), "{scenario}");
            }).await,
            "bounds_conflicts_without_partial_history" => Box::pin(async {
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
                        assert!(attempts <= conflict::RETRIES + 1, "{scenario}");
                        sequence = advance(&other).await;
                        release.send(()).unwrap();
                    }
                    (sequence, attempts)
                };
                let (result, (sequence, attempts)) = tokio::join!(persist, competing);
                assert_eq!(
                    attempts,
                    conflict::RETRIES + 1,
        "{scenario}: history append ended after {attempts} conflicts: {result:?}"
                );
                let error = result.unwrap_err();
                assert!(
                    error.code == ErrorCode::Busy && error.message.contains("storage transaction conflicted"),
        "{scenario}: {error:?}"
                );
                assert!(fixture.database.connection.is_autocommit().unwrap(), "{scenario}");
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
                assert_eq!(count, 0, "{scenario}");
                assert!(
                    !state(&fixture.database.connection, session)
                        .await
                        .unwrap()
                        .contains_key("saved")
                , "{scenario}");
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
                assert_eq!(watermark as u64, sequence, "{scenario}");
                assert!(matches!(
                    published.try_recv(),
                    Err(broadcast::error::TryRecvError::Empty)
                ), "{scenario}");
            }).await,
            "retains_slots_around_provider_context" => Box::pin(async {
                let turn = TurnId::new();
                let mut signed = request("read", "read_file", serde_json::json!({"path":"source.rs"}));
                signed.provider = Some(
                    serde_json::from_value(
                        serde_json::json!({"thoughtSignature":"signature","opaque":{"retained":true}}),
                    )
                    .unwrap(),
                );
                let mut record = message(
        "assistant",
                    Role::Assistant,
                    vec![
                        BlockKind::Text {
                            text: "Before the call".into(),
                        },
                        BlockKind::Text {
                            text: "Engine slot".into(),
                        },
                        BlockKind::ToolRequest(signed),
                    ],
                );
                record.turn = turn;
                record.provenance.branch = "provider-branch".into();
                let db = &fixture.database.connection;
                record.message_mut().unwrap().content.remove(1);
                let entry = project(db, 1, turn, &record).await.unwrap();
                assert_eq!(entry.author, "assistant", "{scenario}");
                assert_eq!(entry.branch, "provider-branch", "{scenario}");
                assert_eq!(entry.timestamp_ms, record.created.timestamp_millis(), "{scenario}");
                assert_eq!(entry.parts.len(), 3, "{scenario}");
                assert_eq!(entry.parts[0], Part::Text("Before the call".into()), "{scenario}");
                assert_eq!(
                    entry.parts[1],
                    Part::Resource(serde_json::json!({"type":"provider_context"}))
                , "{scenario}");
                assert!(matches!(&entry.parts[2], Part::ToolCall { id:Some(id), .. } if id == "read"), "{scenario}");
                let BlockKind::ToolRequest(request) = &record.message().unwrap().content[1].kind else {
                    panic!("{scenario}: tool request expected")
                };
                assert_eq!(
                    request.provider.as_ref().unwrap()["thoughtSignature"],
        "signature"
                , "{scenario}");
                assert_eq!(record.message().unwrap().content[1].index, 2, "{scenario}");
                assert_eq!(
                    tools::raw(db, turn, 1, &record, 2)
                        .await
                        .unwrap()
                        .unwrap()
                        .id
                        .as_deref(),
                    Some("read")
                , "{scenario}");
            }).await,
            _ => unreachable!(),
        }
    }
}

fn captured(turn: TurnId) -> Record {
    let mut event = message(
        "tool",
        Role::User,
        vec![BlockKind::ToolResponse(response(
            "completed-write",
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

async fn advance(connection: &Connection) -> u64 {
    let transaction = conflict::begin(connection).await.unwrap();
    let sequence = counters::next(&transaction, counters::Namespace::AgentEvents)
        .await
        .unwrap();
    conflict::commit(transaction).await.unwrap();
    sequence
}
