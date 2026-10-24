use super::*;
use sailry_protocol::conversation::compaction::{Failure, Outcome, Reason, Source, Stage};

async fn provider(fixture: &mut Fixture) -> Provider {
    let config = crate::store::commands::session(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .config;
    let provider: Provider = serde_json::from_value(serde_json::json!({
        "id":config.provider,"revision":0,"name":"Policy fixture","api":"chat_completions","authentication":"api_key",
        "endpoint":"http://127.0.0.1:1","enabled":true,"default_model":"fixture","credential":null,
        "models":[{"id":"fixture","context":4096,"output":2048,"reasoning":true,"efforts":["low"],"default_effort":"low"}]
    })).unwrap();
    let Output::Provider(provider) = command(
        &mut fixture.database,
        &fixture.events,
        Command::PutProvider {
            provider,
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("provider expected");
    };
    provider
}

#[tokio::test]
async fn uses_captured_policy_until_queued_work_ends() {
    let mut fixture = Fixture::new().await;
    let mut provider = provider(&mut fixture).await;
    let queued = fixture.queued().await;
    provider.models[0].context = 200_000;
    provider.models[0].output = 64_000;
    let expected_revision = provider.revision;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::PutProvider {
            provider,
            expected_revision,
        },
    )
    .await;
    let captured = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .budget
        .unwrap();
    assert_eq!(captured.context_limit, 4096);
    assert_eq!(captured.trigger_tokens, 2048);
    assert_eq!(captured.output_reserve, 2048);
    assert_eq!(captured.summary_output, 512);
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RemoveQueuedTurn {
            turn: queued,
            expected_revision: 1,
        },
    )
    .await;
    let current = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .budget
        .unwrap();
    assert_eq!(current.context_limit, 200_000);
    assert_eq!(current.trigger_tokens, 136_000);
    assert_eq!(current.output_reserve, 64_000);
    assert_eq!(current.summary_output, 8192);
}

#[tokio::test]
async fn locates_maintenance_beyond_loaded_history() {
    let mut fixture = Fixture::new().await;
    let anchor = event(100, 10);
    fixture.append([anchor.clone()]).await;
    let mut summary = AdkEvent::new("summary-fixture");
    summary.author = "system".into();
    summary
        .provider_metadata
        .insert(super::super::super::compaction::END_EVENT.into(), anchor.id);
    summary.actions.compaction = Some(adk_core::EventCompaction {
        start_timestamp: anchor.timestamp,
        end_timestamp: anchor.timestamp,
        compacted_content: adk_core::Content::new("model").with_text("Verified handoff"),
    });
    let summary_id = summary.id.clone();
    let summary_time = summary.timestamp.timestamp_millis();
    let turn = fixture.append([summary]).await;
    for _ in 0..20 {
        fixture.append([event(40, 5)]).await;
    }
    let page = fixture.read(None, 1).await;
    assert!(!page.page.entries.iter().any(|entry| entry.id == summary_id));
    let record = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .last_compaction
        .unwrap();
    assert_eq!(record.id, summary_id);
    assert_eq!(record.turn, turn);
    assert_eq!(
        record.turn_sequence,
        super::super::super::runs::get(&fixture.database.connection, turn)
            .await
            .unwrap()
            .sequence
    );
    assert_eq!(record.timestamp_ms, summary_time);
    assert_eq!(record.outcome, Outcome::Succeeded);
    assert!(record.sequence < page.page.entries[0].sequence);

    let failure = Failure {
        source: Source::Automatic,
        stage: Stage::Summary,
        reason: Reason::TruncatedSummary,
        continued: true,
        status: None,
        cause: None,
    };
    let mut event = AdkEvent::new("failure-fixture");
    event.author = "system".into();
    event.provider_metadata.insert(
        "sailry_compaction_failure".into(),
        serde_json::to_string(&failure).unwrap(),
    );
    let id = event.id.clone();
    fixture.append([event]).await;
    let latest = read(&fixture.database.connection, fixture.session)
        .await
        .unwrap()
        .last_compaction
        .unwrap();
    assert_eq!(latest.id, id);
    assert_eq!(latest.outcome, Outcome::Failed(failure));
    assert!(latest.sequence > record.sequence);
}
