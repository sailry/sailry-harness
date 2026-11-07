use super::*;
use crate::store::agent::tests::fixture::Fixture;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};

mod cache;
pub(super) mod snapshots;

async fn running() -> (Fixture, TurnId) {
    let mut fixture = Fixture::new().await;
    let turn = fixture.queued().await;
    runs::start(&fixture.database.connection, turn)
        .await
        .unwrap();
    assert_eq!(
        runs::claim(&mut fixture.database, &fixture.events)
            .await
            .unwrap()
            .unwrap()
            .turn
            .id,
        turn,
    );
    (fixture, turn)
}

async fn apply(fixture: &mut Fixture, turn: TurnId, changes: Vec<Effect>) -> Result<(), Fault> {
    effects(
        &mut fixture.database,
        fixture.session,
        turn,
        changes,
        Presentation::default(),
        None,
        &fixture.events,
    )
    .await
}

async fn load(fixture: &Fixture, turn: TurnId) -> Session {
    get(
        &fixture.database.connection,
        turn,
        Query {
            session: fixture.session,
            recent: None,
            after: None,
        },
    )
    .await
    .unwrap()
}

fn usage() -> model::Usage {
    model::Usage {
        model: Some("fixture".into()),
        input: Some(12),
        output: Some(3),
        total: Some(15),
        reasoning: Some(2),
        ..model::Usage::default()
    }
}

fn summary(after: &str) -> ModelMessage {
    let mut message = ModelMessage::user()
        .with_generated_id()
        .with_text("Tool summary")
        .with_visibility(false, true);
    message
        .metadata
        .set_operation_note("tool_pair_compaction", "after", json!(after));
    message
}

#[tokio::test]
async fn retains_names_outside_requested_history() {
    let (mut fixture, turn) = running().await;
    apply(
        &mut fixture,
        turn,
        vec![
            ModelMessage::assistant()
                .with_generated_id()
                .with_tool_request("old", Ok(CallToolRequestParams::new("read_file")))
                .with_visibility(false, false)
                .into(),
            ModelMessage::user()
                .with_generated_id()
                .with_tool_response("old", Ok(CallToolResult::success(vec![])))
                .with_visibility(false, false)
                .into(),
        ],
    )
    .await
    .unwrap();
    let session = get(
        &fixture.database.connection,
        turn,
        Query {
            session: fixture.session,
            recent: Some(0),
            after: Some(Utc::now() + chrono::Duration::days(1)),
        },
    )
    .await
    .unwrap();
    assert!(session.records.is_empty());
    assert_eq!(session.tool_names, ["read_file".into()].into());
}

#[tokio::test]
async fn keeps_summary_before_current_output() {
    for failed in [false, true] {
        let (mut fixture, turn) = running().await;
        let request = ModelMessage::assistant()
            .with_generated_id()
            .with_tool_request("call", Ok(CallToolRequestParams::new("read_file")));
        let response = ModelMessage::user().with_generated_id().with_tool_response(
            "call",
            Ok(CallToolResult::success(vec![ContentBlock::text(
                "Raw tool output",
            )])),
        );
        let request_id = request.id.clone().unwrap();
        let response_id = response.id.clone().unwrap();
        let terminal = if failed {
            ModelMessage::assistant()
                .with_generated_id()
                .with_error(model::ErrorKind::Authentication, "Authentication failed")
        } else {
            ModelMessage::assistant()
                .with_generated_id()
                .with_text("Final answer")
        };
        let terminal_id = terminal.id.clone().unwrap();
        let next = ModelMessage::user()
            .with_generated_id()
            .with_text("Next task");
        let next_id = next.id.clone().unwrap();
        apply(
            &mut fixture,
            turn,
            vec![
                ModelMessage::user()
                    .with_generated_id()
                    .with_text("Old task")
                    .into(),
                request.into(),
                response.into(),
                next.into(),
                terminal.into(),
            ],
        )
        .await
        .unwrap();
        let before = load(&fixture, turn).await;
        let source = before
            .records
            .iter()
            .find(|record| record.id == response_id)
            .unwrap();
        let source_time = source.created;
        let summary = summary(&response_id);
        let summary_id = summary.id.clone().unwrap();
        let mut charge = Record::new(
            turn,
            Kind::Message(crate::agent::canonical::Message {
                model: Some("fixture".into()),
                usage: Some(conversion::provider_usage(&usage()).unwrap()),
                ..Default::default()
            }),
        );
        charge.provenance.author = "system".into();
        charge.metadata.runtime = Some(Runtime {
            user_visible: false,
            agent_visible: false,
            ..Default::default()
        });
        let charge_id = charge.id.clone();
        apply(
            &mut fixture,
            turn,
            vec![
                ConversationEffect::SetMessageVisibility {
                    message_id: request_id.clone(),
                    user_visible: true,
                    agent_visible: false,
                }
                .into(),
                ConversationEffect::SetMessageVisibility {
                    message_id: response_id.clone(),
                    user_visible: true,
                    agent_visible: false,
                }
                .into(),
                summary.into(),
                Effect::Records(vec![charge]),
            ],
        )
        .await
        .unwrap();
        let session = load(&fixture, turn).await;
        let ids: Vec<_> = session
            .conversation
            .iter()
            .filter_map(|message| message.id.as_deref())
            .collect();
        let source_index = ids.iter().position(|id| *id == response_id).unwrap();
        assert_eq!(ids[source_index + 1], summary_id);
        assert_eq!(ids[source_index + 2], next_id);
        assert_eq!(ids.last().copied(), Some(terminal_id.as_str()));
        assert!(!ids.contains(&charge_id.as_str()));
        assert!(
            session
                .records
                .iter()
                .any(|record| record.id == charge_id && record.usage().unwrap().input == Some(12))
        );
        assert_eq!(
            session
                .records
                .iter()
                .find(|record| record.id == summary_id)
                .unwrap()
                .created,
            source_time
        );
        assert_eq!(
            session.records[..before.records.len()]
                .iter()
                .map(|record| &record.id)
                .collect::<Vec<_>>(),
            before
                .records
                .iter()
                .map(|record| &record.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            session.conversation.last().unwrap().error_kind().is_some(),
            failed
        );
        assert_eq!(
            crate::agent::engine::conversation::ends_turn(&session.conversation),
            !failed
        );
        let projected = session
            .conversation
            .iter()
            .filter(|message| message.metadata.agent_visible)
            .collect::<Vec<_>>();
        assert!(
            !projected
                .iter()
                .any(|message| message.id.as_deref() == Some(request_id.as_str())
                    || message.id.as_deref() == Some(response_id.as_str()))
        );
        let public = fixture.read(None, 100).await;
        assert!(
            public
                .page
                .entries
                .iter()
                .any(|entry| entry.id == request_id)
        );
        assert!(
            public
                .page
                .entries
                .iter()
                .any(|entry| entry.id == response_id)
        );
        assert!(
            !public
                .page
                .entries
                .iter()
                .any(|entry| entry.id == summary_id || entry.id == charge_id)
        );
        let bounded = get(
            &fixture.database.connection,
            turn,
            Query {
                session: fixture.session,
                recent: Some(2),
                after: None,
            },
        )
        .await
        .unwrap();
        assert!(bounded.conversation.is_empty());
        runs::finish(
            &mut fixture.database,
            turn,
            if failed {
                sailry_protocol::conversation::Status::Failed
            } else {
                sailry_protocol::conversation::Status::Completed
            },
            None,
            &fixture.events,
        )
        .await
        .unwrap();
        let fork = fixture.fork(fixture.session, turn).await;
        fixture.session = fork.id;
        let fork_turn = fixture.queued().await;
        let forked = load(&fixture, fork_turn).await;
        assert_eq!(forked.conversation, session.conversation);
    }
}

#[tokio::test]
async fn rolls_back_visibility_with_rejected_summary() {
    let (mut fixture, turn) = running().await;
    let original = ModelMessage::assistant()
        .with_generated_id()
        .with_text("Original output");
    let id = original.id.clone().unwrap();
    apply(&mut fixture, turn, vec![original.into()])
        .await
        .unwrap();
    let before = load(&fixture, turn).await;
    let mut events = fixture.events.subscribe();
    let feed_sequence = fixture.database.feeds.sequence(fixture.session);
    let rejected = summary(&id).with_image("invalid base64!", "image/png");
    assert!(
        apply(
            &mut fixture,
            turn,
            vec![
                ConversationEffect::SetMessageVisibility {
                    message_id: id,
                    user_visible: false,
                    agent_visible: false
                }
                .into(),
                rejected.into(),
            ]
        )
        .await
        .is_err()
    );
    let after = load(&fixture, turn).await;
    assert_eq!(after.records.len(), before.records.len());
    assert_eq!(after.conversation, before.conversation);
    assert!(matches!(
        events.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
    assert_eq!(
        fixture.database.feeds.sequence(fixture.session),
        feed_sequence
    );
}

#[tokio::test]
async fn retains_error_only_usage() {
    let (mut fixture, turn) = running().await;
    let error = ModelMessage::assistant()
        .with_generated_id()
        .with_error(model::ErrorKind::Other, "Stream failed");
    let id = error.id.clone().unwrap();
    apply(
        &mut fixture,
        turn,
        vec![error.into(), Effect::Usage(usage())],
    )
    .await
    .unwrap();
    let session = load(&fixture, turn).await;
    let record = session
        .records
        .iter()
        .find(|record| record.id == id)
        .unwrap();
    let stored = record.usage().unwrap();
    assert_eq!(stored.reasoning, Some(2));
    assert_eq!(
        (stored.input, stored.output, stored.total),
        (Some(12), Some(3), Some(15))
    );
    let projected = session.conversation.last().unwrap();
    assert!(projected.error_kind().is_some());
    assert_eq!(projected.metadata.usage.as_deref().unwrap().input, Some(12));
}

#[tokio::test]
async fn retains_each_attempt() {
    for failed in [false, true] {
        let (mut fixture, turn) = running().await;
        let answer = ModelMessage::assistant().with_generated_id();
        let answer = if failed {
            answer.with_error(model::ErrorKind::Other, "Stream failed")
        } else {
            answer.with_text("Final answer")
        };
        let id = answer.id.clone().unwrap();
        let completed = model::Usage {
            model: Some("fixture".into()),
            input: Some(20),
            output: Some(5),
            total: Some(25),
            ..model::Usage::default()
        };
        apply(
            &mut fixture,
            turn,
            vec![
                Effect::Usage(usage()),
                Effect::Usage(usage()),
                answer.into(),
                Effect::Usage(completed),
            ],
        )
        .await
        .unwrap();
        let session = load(&fixture, turn).await;
        let records: Vec<_> = session
            .records
            .iter()
            .filter(|record| record.usage().is_some())
            .collect();
        assert_eq!(records.len(), 3);
        assert_ne!(records[0].id, records[1].id);
        for record in &records[..2] {
            let runtime = record.metadata.runtime.as_ref().unwrap();
            assert!(!runtime.user_visible);
            assert!(!runtime.agent_visible);
            let usage = record.usage().unwrap();
            assert_eq!((usage.input, usage.output), (Some(12), Some(3)));
            assert_eq!(usage.reasoning, Some(2));
        }
        assert_eq!(records[2].id, id);
        let usage = records[2].usage().unwrap();
        assert_eq!((usage.input, usage.output), (Some(20), Some(5)));
        assert_eq!(session.conversation.len(), 1);
        let projected = session.conversation.last().unwrap();
        assert_eq!(projected.id.as_deref(), Some(id.as_str()));
        assert_eq!(projected.error_kind().is_some(), failed);
        let usage = projected.metadata.usage.as_deref().unwrap();
        assert_eq!((usage.input, usage.output), (Some(20), Some(5)));
        let statistics =
            crate::store::agent::statistics::read(&fixture.database.connection, fixture.session)
                .await
                .unwrap();
        assert_eq!(statistics.responses, 3);
        let usage = statistics.usage.unwrap();
        assert_eq!((usage.input, usage.output, usage.reasoning), (44, 11, 4));
    }
}

#[tokio::test]
async fn retains_usage_without_output() {
    let (mut fixture, turn) = running().await;
    let input = ModelMessage::user()
        .with_generated_id()
        .with_text("Current task");
    let id = input.id.clone().unwrap();
    apply(&mut fixture, turn, vec![input.into()]).await.unwrap();
    apply(
        &mut fixture,
        turn,
        vec![Effect::Usage(usage()), Effect::Usage(usage())],
    )
    .await
    .unwrap();
    let session = load(&fixture, turn).await;
    assert_eq!(
        session.conversation.last().unwrap().id.as_deref(),
        Some(id.as_str())
    );
    let records: Vec<_> = session
        .records
        .iter()
        .filter(|record| record.usage().is_some())
        .collect();
    assert_eq!(records.len(), 2);
    assert_ne!(records[0].id, records[1].id);
    for record in records {
        assert_eq!(record.usage().unwrap().total, Some(15));
        assert_eq!(record.usage().unwrap().reasoning, Some(2));
        let runtime = record.metadata.runtime.as_ref().unwrap();
        assert!(!runtime.user_visible);
        assert!(!runtime.agent_visible);
    }
}

#[tokio::test]
async fn retains_completed_and_compacted_reasoning() {
    let (mut fixture, turn) = running().await;
    let answer = ModelMessage::assistant()
        .with_generated_id()
        .with_text("Final answer");
    let id = answer.id.clone().unwrap();
    apply(
        &mut fixture,
        turn,
        vec![
            ModelMessage::user()
                .with_generated_id()
                .with_text("Task")
                .into(),
            answer.into(),
            Effect::Usage(usage()),
        ],
    )
    .await
    .unwrap();
    let session = load(&fixture, turn).await;
    assert_eq!(
        session
            .records
            .iter()
            .find(|record| record.id == id)
            .unwrap()
            .usage()
            .unwrap()
            .reasoning,
        Some(2)
    );
    let summary = ModelMessage::user()
        .with_generated_id()
        .with_text("Compacted handoff");
    let id = summary.id.clone().unwrap();
    apply(
        &mut fixture,
        turn,
        vec![Effect::Compaction {
            summary: Box::new(summary),
            usage: usage(),
            files: Vec::new(),
            tools: Vec::new(),
        }],
    )
    .await
    .unwrap();
    let restored = load(&fixture, turn).await;
    let record = restored
        .records
        .iter()
        .find(|record| record.id == id)
        .unwrap();
    assert!(matches!(record.kind, Kind::Compaction(_)));
    assert_eq!(record.usage().unwrap().reasoning, Some(2));
    let decoded: Record = serde_json::from_slice(&serde_json::to_vec(record).unwrap()).unwrap();
    assert_eq!(decoded.usage().unwrap().reasoning, Some(2));
}

#[tokio::test]
async fn replaces_loaded_tools_at_compaction() {
    let (mut fixture, turn) = running().await;
    let calls = |names: &[&str]| -> Vec<Effect> {
        let mut request = ModelMessage::assistant().with_generated_id();
        let mut response = ModelMessage::user().with_generated_id();
        for name in names {
            let id = format!("{name}-call");
            request = request
                .with_tool_request(id.clone(), Ok(CallToolRequestParams::new(name.to_string())));
            response = response.with_tool_response(id, Ok(CallToolResult::success(vec![])));
        }
        vec![request.into(), response.into()]
    };
    let mut changes: Vec<Effect> = vec![
        ModelMessage::user()
            .with_generated_id()
            .with_text("Task")
            .into(),
    ];
    changes.extend(calls(&["alpha", "beta"]));
    apply(&mut fixture, turn, changes).await.unwrap();
    assert_eq!(
        load(&fixture, turn).await.context.loaded_tools,
        Some(vec!["alpha".into(), "beta".into()])
    );
    let summary = ModelMessage::user()
        .with_generated_id()
        .with_text("Handoff");
    let id = summary.id.clone().unwrap();
    apply(
        &mut fixture,
        turn,
        vec![Effect::Compaction {
            summary: Box::new(summary),
            usage: usage(),
            files: Vec::new(),
            tools: vec!["beta".into()],
        }],
    )
    .await
    .unwrap();
    let compacted = load(&fixture, turn).await;
    assert_eq!(compacted.context.loaded_tools, Some(vec!["beta".into()]));
    let record = compacted
        .records
        .iter()
        .find(|record| record.id == id)
        .unwrap();
    assert!(!record.context().unwrap().captured);
    apply(&mut fixture, turn, calls(&["gamma"])).await.unwrap();
    let expected = Some(vec!["beta".into(), "gamma".into()]);
    assert_eq!(load(&fixture, turn).await.context.loaded_tools, expected);
    history::rebuild_session(&fixture.database.connection, fixture.session)
        .await
        .unwrap();
    assert_eq!(load(&fixture, turn).await.context.loaded_tools, expected);
}

#[tokio::test]
async fn keeps_precise_receipt_time_on_retry() {
    let (mut fixture, turn) = running().await;
    let mut message = ModelMessage::assistant()
        .with_generated_id()
        .with_text("Current output");
    message.created = 1;
    let started = Utc::now();
    apply(&mut fixture, turn, vec![message.clone().into()])
        .await
        .unwrap();
    let before = load(&fixture, turn).await;
    assert!(before.records[0].created >= started && before.records[0].created <= Utc::now());
    apply(&mut fixture, turn, vec![message.into()])
        .await
        .unwrap();
    let after = load(&fixture, turn).await;
    assert_eq!(after.records.len(), 1);
    assert_eq!(before.records[0].created, after.records[0].created);
}
