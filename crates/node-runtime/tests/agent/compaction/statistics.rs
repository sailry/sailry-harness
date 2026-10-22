use super::*;

#[tokio::test]
async fn tracks_policy_observation_and_maintenance() {
    for remote in [false, true] {
        let server = Server::compaction(false).await;
        let fixture = Fixture::files(remote, &server).await;
        std::fs::write(
            fixture.root.join("source.txt"),
            "Verified source facts\n".repeat(200),
        )
        .unwrap();
        let mut updates = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let Update::ConversationSnapshot(initial) = updates.next().await.unwrap() else {
            panic!("snapshot expected");
        };
        assert_eq!(initial.statistics.context_tokens, None);
        assert_eq!(initial.statistics.context_limit, None);
        assert_eq!(initial.statistics.context_estimate, None);
        assert!(initial.statistics.budget.is_some());
        submit(
            &fixture.client,
            fixture.session.id,
            "Read source.txt".into(),
        )
        .await;
        let observed = super::budget::statistics(&fixture).await;
        assert!(observed.context_tokens.is_some());
        assert!(observed.context_timestamp_ms.is_some());
        let mut provider = configuration::snapshot(&fixture.client).await.providers[0].clone();
        let mut switched = provider.models[0].clone();
        switched.id = "switched-model".into();
        switched.context = 4096;
        switched.output = 512;
        provider.models.push(switched);
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        let mut config = fixture.session.config.clone();
        config.model = "switched-model".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: 1,
                config,
            }))
            .await
            .unwrap();
        let changed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Update::ConversationFrame(frame) = updates.next().await.unwrap()
                    && let sailry_protocol::conversation::Change::Statistics(statistics) =
                        frame.change
                    && statistics
                        .budget
                        .as_ref()
                        .is_some_and(|budget| budget.model == "switched-model")
                {
                    break statistics;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(changed.context_tokens, observed.context_tokens);
        assert_eq!(changed.context_limit, observed.context_limit);
        assert_eq!(changed.context_model, observed.context_model);
        assert_eq!(changed.context_timestamp_ms, observed.context_timestamp_ms);
        let policy = changed.budget.unwrap();
        assert_eq!(policy.context_limit, 4096);
        assert_eq!(policy.output_reserve, 512);
        assert_eq!(policy.trigger_tokens, 3276);
        assert_eq!(policy.summary_output, 512);
        assert_eq!(policy.summary_input, 3584);
        drop(updates);
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::CompactContext {
                session: fixture.session.id,
                expected_revision: 2,
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected");
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed
        );
        let compacted = super::budget::statistics(&fixture).await;
        assert_eq!(compacted.context_tokens, None);
        assert!(compacted.context_estimate.is_some());
        let record = compacted.last_compaction.unwrap();
        assert_eq!(record.turn, turn.id);
        assert_eq!(
            record.outcome,
            sailry_protocol::conversation::compaction::Outcome::Succeeded
        );
        assert!(
            page.entries
                .iter()
                .any(|entry| entry.id == record.id && entry.sequence == record.sequence)
        );
        let Output::QueuedTurn(next) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: 2,
                message: "Continue from the handoff".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected");
        };
        finished(&fixture.client, fixture.session.id, next.id).await;
        let latest = super::budget::statistics(&fixture).await;
        assert!(latest.context_tokens.is_some());
        assert_eq!(latest.context_estimate, None);
        assert_eq!(latest.context_model.as_deref(), Some("switched-model"));
        assert_eq!(latest.context_limit, Some(4096));
        assert_eq!(latest.last_compaction, Some(record.clone()));
        let Output::TurnHistory(found) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadTurn {
                session: fixture.session.id,
                turn: record.turn,
                expected_revision: page.revision,
                before: Some(record.sequence + 1),
                limit: 1,
            }))
            .await
            .unwrap()
        else {
            panic!("turn history expected");
        };
        assert!(found.entries.iter().any(|entry| entry.id == record.id));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
