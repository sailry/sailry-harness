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
async fn preserves_maintenance_observations() {
    let mut fixture = Fixture::new().await;
    for (index, scenario) in [
        "uses_captured_policy_until_queued_work_ends",
        "locates_maintenance_beyond_loaded_history",
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            fixture.fresh_session().await;
        }
        eprintln!("scenario: {scenario}");
        match scenario {
            "uses_captured_policy_until_queued_work_ends" => {
                Box::pin(async {
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
                    assert_eq!(captured.context_limit, 4096, "{scenario}");
                    assert_eq!(captured.trigger_tokens, 3276, "{scenario}");
                    assert_eq!(captured.output_reserve, 2048, "{scenario}");
                    assert_eq!(captured.summary_output, 2048, "{scenario}");
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
                    assert_eq!(current.context_limit, 200_000, "{scenario}");
                    assert_eq!(current.trigger_tokens, 160_000, "{scenario}");
                    assert_eq!(current.output_reserve, 64_000, "{scenario}");
                    // Compaction handoffs stay bounded below the model's full output limit.
                    assert_eq!(current.summary_output, 8_000, "{scenario}");
                })
                .await
            }
            "locates_maintenance_beyond_loaded_history" => {
                Box::pin(async {
                    let anchor = event(100, 10);
                    fixture.append([anchor.clone()]).await;
                    let summary = summary(&anchor, "Verified handoff");
                    let summary_id = summary.id.clone();
                    let summary_time = summary.created.timestamp_millis();
                    let turn = fixture.append([summary]).await;
                    for _ in 0..20 {
                        fixture.append([event(40, 5)]).await;
                    }
                    let page = fixture.read(None, 1).await;
                    assert!(
                        !page.page.entries.iter().any(|entry| entry.id == summary_id),
                        "{scenario}"
                    );
                    let record = read(&fixture.database.connection, fixture.session)
                        .await
                        .unwrap()
                        .last_compaction
                        .unwrap();
                    assert_eq!(record.id, summary_id, "{scenario}");
                    assert_eq!(record.turn, turn, "{scenario}");
                    assert_eq!(
                        record.turn_sequence,
                        super::super::super::runs::get(&fixture.database.connection, turn)
                            .await
                            .unwrap()
                            .sequence,
                        "{scenario}"
                    );
                    assert_eq!(record.timestamp_ms, summary_time, "{scenario}");
                    assert_eq!(record.outcome, Outcome::Succeeded, "{scenario}");
                    assert!(
                        record.sequence < page.page.entries[0].sequence,
                        "{scenario}"
                    );

                    let failure = Failure {
                        source: Source::Automatic,
                        stage: Stage::Summary,
                        reason: Reason::TruncatedSummary,
                        continued: true,
                        status: None,
                        cause: None,
                    };
                    let mut event = crate::store::agent::tests::fixture::record(
                        "system",
                        canonical::Kind::Message(canonical::Message::default()),
                    );
                    event.metadata.compaction = Some(canonical::Maintenance {
                        failure: Some(failure.clone()),
                        ..Default::default()
                    });
                    let id = event.id.clone();
                    fixture.append([event]).await;
                    let latest = read(&fixture.database.connection, fixture.session)
                        .await
                        .unwrap()
                        .last_compaction
                        .unwrap();
                    assert_eq!(latest.id, id, "{scenario}");
                    assert_eq!(latest.outcome, Outcome::Failed(failure), "{scenario}");
                    assert!(latest.sequence > record.sequence, "{scenario}");
                })
                .await
            }
            _ => unreachable!(),
        }
    }
}
