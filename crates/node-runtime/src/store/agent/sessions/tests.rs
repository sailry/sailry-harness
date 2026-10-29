use super::*;
use crate::store::agent::tests::fixture::Fixture;
use goose_provider_types::conversation::{
    message::MessageErrorKind,
    token_usage::{ProviderUsage, Usage as GdkUsage},
};
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};

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

fn usage() -> ProviderUsage {
    let mut usage =
        ProviderUsage::new("fixture".into(), GdkUsage::new(Some(12), Some(3), Some(15)));
    usage.additional_data = Some(
        [("reasoning_tokens".into(), json!(2))]
            .into_iter()
            .collect(),
    );
    usage
}

fn summary(after: &str) -> GdkMessage {
    let mut message = GdkMessage::user()
        .with_generated_id()
        .with_text("Tool summary")
        .with_visibility(false, true);
    message
        .metadata
        .set_operation_note("tool_pair_compaction", "after", json!(after));
    message
}

#[tokio::test]
async fn keeps_summary_before_current_output() {
    for failed in [false, true] {
        let (mut fixture, turn) = running().await;
        let request = GdkMessage::assistant()
            .with_generated_id()
            .with_tool_request("call", Ok(CallToolRequestParams::new("read_file")));
        let response = GdkMessage::user().with_generated_id().with_tool_response(
            "call",
            Ok(CallToolResult::success(vec![ContentBlock::text(
                "Raw tool output",
            )])),
        );
        let request_id = request.id.clone().unwrap();
        let response_id = response.id.clone().unwrap();
        let terminal = if failed {
            GdkMessage::assistant()
                .with_generated_id()
                .with_error(MessageErrorKind::Authentication, "Authentication failed")
        } else {
            GdkMessage::assistant()
                .with_generated_id()
                .with_text("Final answer")
        };
        let terminal_id = terminal.id.clone().unwrap();
        let next = GdkMessage::user()
            .with_generated_id()
            .with_text("Next task");
        let next_id = next.id.clone().unwrap();
        apply(
            &mut fixture,
            turn,
            vec![
                GdkMessage::user()
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
            .messages()
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
            goose_agent::operation::ends_turn(session.conversation.messages()),
            !failed
        );
        let projected = session.conversation.agent_visible_messages();
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
        assert!(bounded.conversation.messages().is_empty());
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
    let original = GdkMessage::assistant()
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
    let error = GdkMessage::assistant()
        .with_generated_id()
        .with_error(MessageErrorKind::Other, "Stream failed");
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
    assert_eq!(
        projected.metadata.usage.as_deref().unwrap().input_tokens,
        Some(12)
    );
}

#[tokio::test]
async fn retains_usage_without_output() {
    let (mut fixture, turn) = running().await;
    let input = GdkMessage::user()
        .with_generated_id()
        .with_text("Current task");
    let id = input.id.clone().unwrap();
    apply(&mut fixture, turn, vec![input.into()]).await.unwrap();
    apply(&mut fixture, turn, vec![Effect::Usage(usage())])
        .await
        .unwrap();
    let session = load(&fixture, turn).await;
    assert_eq!(
        session.conversation.last().unwrap().id.as_deref(),
        Some(id.as_str())
    );
    assert_eq!(
        session.records.last().unwrap().usage().unwrap().total,
        Some(15)
    );
    assert_eq!(
        session.records.last().unwrap().usage().unwrap().reasoning,
        Some(2)
    );
    assert!(
        !session
            .records
            .last()
            .unwrap()
            .metadata
            .runtime
            .as_ref()
            .unwrap()
            .user_visible
    );
}

#[tokio::test]
async fn retains_completed_and_compacted_reasoning() {
    let (mut fixture, turn) = running().await;
    let answer = GdkMessage::assistant()
        .with_generated_id()
        .with_text("Final answer");
    let id = answer.id.clone().unwrap();
    apply(
        &mut fixture,
        turn,
        vec![
            GdkMessage::user()
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
    let summary = GdkMessage::user()
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
async fn keeps_precise_receipt_time_on_retry() {
    let (mut fixture, turn) = running().await;
    let mut message = GdkMessage::assistant()
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
