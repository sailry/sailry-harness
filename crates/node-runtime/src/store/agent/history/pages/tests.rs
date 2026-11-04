use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

#[tokio::test]
async fn records_turns() {
    let mut fixture = Fixture::new().await;
    for (index, scenario) in [
        "fits_cached_entry_lengths_exactly",
        "preserves_turn_boundaries",
        "chunks_complete_turns",
        "validates_resource_boundaries_and_limits",
        "bounds_failed_inputs_without_truncation",
        "defers_turns_above_a_partial_turn",
        "seeks_turn_events_without_sorting",
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            fixture.fresh_session().await;
        }
        eprintln!("scenario: {scenario}");
        match scenario {
            "fits_cached_entry_lengths_exactly" => {
                Box::pin(async {
                    let turn = fixture.completed(&["escaped \"\n", "中文🙂", "tail"]).await;
                    let mut page = fixture.read(None, 1).await.page;
                    let entries = page.entries.clone();
                    let bytes = entries
                        .iter()
                        .map(|entry| encode(entry).unwrap().len())
                        .sum::<usize>();
                    assert_eq!(
                        encoded_size(&mut page, bytes).unwrap(),
                        encode(&page).unwrap().len(),
                        "{scenario}"
                    );
                    let budget =
                        encode(&page).unwrap().len() - encode(&entries[0]).unwrap().len() - 1;
                    let headroom = MAX_FRAME_BYTES - 64 * 1024 - budget;
                    assert_eq!(
                        fit(&mut page, headroom, true).unwrap(),
                        BTreeSet::from([turn]),
                        "{scenario}"
                    );
                    assert_eq!(page.entries.as_slice(), &entries[1..], "{scenario}");
                    assert_eq!(encode(&page).unwrap().len(), budget, "{scenario}");
                    std::sync::Arc::make_mut(&mut page.entries).clear();
                    assert_eq!(
                        encoded_size(&mut page, 0).unwrap(),
                        encode(&page).unwrap().len(),
                        "{scenario}"
                    );
                })
                .await
            }
            "preserves_turn_boundaries" => {
                Box::pin(async {
                    let first = fixture
                        .completed(&["first prompt", "first call", "first result", "first answer"])
                        .await;
                    let second = fixture.completed(&["second prompt", "second answer"]).await;
                    let third = fixture.completed(&["third prompt", "third answer"]).await;
                    let queued = fixture.queued().await;
                    let cancelled = fixture.queued().await;
                    command(
                        &mut fixture.database,
                        &fixture.events,
                        Command::RemoveQueuedTurn {
                            turn: cancelled,
                            expected_revision: 1,
                        },
                    )
                    .await;
                    let recent = fixture.read(None, 1).await;
                    assert!(recent.missing.is_empty(), "{scenario}");
                    assert_eq!(recent.page.next_before, Some(third), "{scenario}");
                    assert_eq!(
                        recent
                            .page
                            .runs
                            .iter()
                            .map(|run| run.turn)
                            .collect::<Vec<_>>(),
                        [third, queued, cancelled],
                        "{scenario}"
                    );
                    assert_eq!(recent.page.entries.len(), 2, "{scenario}");
                    assert!(
                        recent.page.entries.iter().all(|entry| entry.turn == third),
                        "{scenario}"
                    );
                    assert_eq!(recent.page.queue.items.len(), 1, "{scenario}");
                    assert_eq!(recent.page.queue.items[0].turn, queued, "{scenario}");
                    let older = fixture.read(recent.page.next_before, 1).await;
                    assert_eq!(older.page.next_before, Some(second), "{scenario}");
                    assert_eq!(older.page.runs.len(), 1, "{scenario}");
                    assert!(
                        older.page.entries.iter().all(|entry| entry.turn == second),
                        "{scenario}"
                    );
                    let oldest = fixture.read(older.page.next_before, 1).await;
                    assert_eq!(oldest.page.next_before, None, "{scenario}");
                    assert_eq!(oldest.page.entries.len(), 4, "{scenario}");
                    assert!(
                        oldest.page.entries.iter().all(|entry| entry.turn == first),
                        "{scenario}"
                    );
                    let combined = fixture.read(Some(third), 2).await;
                    assert_eq!(combined.page.entries.len(), 6, "{scenario}");
                    assert_eq!(
                        combined
                            .page
                            .runs
                            .iter()
                            .map(|run| run.turn)
                            .collect::<Vec<_>>(),
                        [first, second],
                        "{scenario}"
                    );
                    assert_eq!(combined.page.next_before, None, "{scenario}");
                })
                .await
            }
            "chunks_complete_turns" => {
                Box::pin(async {
                    let text = "中文🙂".repeat(90_000);
                    let large = fixture.completed(&[text.as_str(); 7]).await;
                    let recent = fixture.read(None, 1).await;
                    assert_eq!(recent.page.next_before, None, "{scenario}");
                    assert_eq!(recent.missing, [large], "{scenario}");
                    assert!(
                        encode(&Output::Conversation(recent.clone())).unwrap().len()
                            < MAX_FRAME_BYTES,
                        "{scenario}"
                    );
                    let mut restored = std::sync::Arc::unwrap_or_clone(recent.page.entries);
                    let mut chunks = 0;
                    loop {
                        let before = restored.first().unwrap().sequence;
                        let chunk = turn(
                            &fixture.database.connection,
                            fixture.session,
                            large,
                            Some(before),
                            100,
                        )
                        .await
                        .unwrap();
                        assert!(!chunk.entries.is_empty(), "{scenario}");
                        assert!(
                            chunk
                                .entries
                                .iter()
                                .all(|entry| entry.turn == large && entry.sequence < before),
                            "{scenario}"
                        );
                        assert!(
                            encode(&Output::TurnHistory(chunk.clone())).unwrap().len()
                                < MAX_FRAME_BYTES,
                            "{scenario}"
                        );
                        let next = chunk.next_before;
                        let mut entries = chunk.entries;
                        entries.append(&mut restored);
                        restored = entries;
                        chunks += 1;
                        if next.is_none() {
                            break;
                        }
                        assert_eq!(next, Some(restored[0].sequence), "{scenario}");
                        assert!(chunks < 7, "{scenario}");
                    }
                    assert_eq!(chunks, 2, "{scenario}");
                    assert_eq!(restored.len(), 7, "{scenario}");
                    assert!(
                        restored
                            .windows(2)
                            .all(|pair| pair[0].sequence < pair[1].sequence),
                        "{scenario}"
                    );
                    assert!(
                        restored
                            .iter()
                            .all(|entry| entry.parts == [Part::Text(text.clone())]),
                        "{scenario}"
                    );

                    let many = fixture.completed(&vec!["small event"; 105]).await;
                    let recent = fixture.read(None, 1).await;
                    assert_eq!(recent.page.runs[0].turn, many, "{scenario}");
                    assert_eq!(recent.missing, [many], "{scenario}");
                    assert_eq!(recent.page.entries.len(), 100, "{scenario}");
                    assert_eq!(recent.page.next_before, Some(many), "{scenario}");
                    let chunk = turn(
                        &fixture.database.connection,
                        fixture.session,
                        many,
                        Some(recent.page.entries[0].sequence),
                        100,
                    )
                    .await
                    .unwrap();
                    assert_eq!(chunk.entries.len(), 5, "{scenario}");
                    assert_eq!(chunk.next_before, None, "{scenario}");
                })
                .await
            }
            "validates_resource_boundaries_and_limits" => {
                Box::pin(async {
                    let first = fixture.completed(&[]).await;
                    let original = fixture.read(None, 1).await;
                    assert_eq!(original.page.runs[0].turn, first, "{scenario}");
                    assert!(original.page.entries.is_empty(), "{scenario}");
                    let other = SessionId::new();
                    assert_eq!(
                        turn(&fixture.database.connection, other, first, None, 1)
                            .await
                            .unwrap_err()
                            .code,
                        ErrorCode::WrongTarget,
                        "{scenario}"
                    );
                    assert!(
                        read(
                            &fixture.database.connection,
                            fixture.session,
                            Some(TurnId::new()),
                            1
                        )
                        .await
                        .is_err(),
                        "{scenario}"
                    );
                    assert!(
                        read(&fixture.database.connection, fixture.session, None, 0)
                            .await
                            .is_err(),
                        "{scenario}"
                    );
                    assert!(
                        turn(
                            &fixture.database.connection,
                            fixture.session,
                            first,
                            None,
                            0
                        )
                        .await
                        .is_err(),
                        "{scenario}"
                    );
                    for limit in [101, u16::MAX] {
                        assert_eq!(
                            read(&fixture.database.connection, fixture.session, None, limit)
                                .await
                                .unwrap(),
                            original,
                            "{scenario}"
                        );
                        assert!(
                            turn(
                                &fixture.database.connection,
                                fixture.session,
                                first,
                                None,
                                limit
                            )
                            .await
                            .unwrap()
                            .entries
                            .is_empty(),
                            "{scenario}"
                        );
                    }
                    assert!(
                        turn(
                            &fixture.database.connection,
                            fixture.session,
                            first,
                            Some(u64::MAX),
                            1
                        )
                        .await
                        .is_err(),
                        "{scenario}"
                    );
                    assert_eq!(fixture.read(None, 1).await, original, "{scenario}");
                })
                .await
            }
            "bounds_failed_inputs_without_truncation" => {
                Box::pin(async {
                    // JSON expands these source bytes sixfold; the logical page cannot fit all captures.
                    let input = format!("Task {}", "\u{0001}".repeat(1024 * 1024 - 5));
                    let mut expected = Vec::new();
                    for _ in 0..5 {
                        let turn = fixture.queued().await;
                        // Each failed run pauses dispatch; explicitly resume the next fixture input.
                        super::super::queue::pause(
                            &fixture.database.connection,
                            fixture.session,
                            false,
                        )
                        .await
                        .unwrap();
                        command(
                            &mut fixture.database,
                            &fixture.events,
                            Command::EditQueuedTurn {
                                turn,
                                expected_revision: 1,
                                message: input.clone().into(),
                            },
                        )
                        .await;
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
                            "{scenario}"
                        );
                        runs::finish(
                            &mut fixture.database,
                            turn,
                            Status::Failed,
                            None,
                            &fixture.events,
                        )
                        .await
                        .unwrap();
                        expected.push(turn);
                    }
                    let queued = fixture.queued().await;
                    let mut before = None;
                    let mut restored = BTreeSet::new();
                    loop {
                        let history = fixture.read(before, 100).await;
                        assert!(
                            encode(&Output::Conversation(history.clone()))
                                .unwrap()
                                .len()
                                < MAX_FRAME_BYTES,
                            "{scenario}"
                        );
                        assert!(history.page.entries.is_empty(), "{scenario}");
                        assert!(history.missing.is_empty(), "{scenario}");
                        assert!(
                            history
                                .page
                                .queue
                                .items
                                .iter()
                                .any(|item| item.turn == queued),
                            "{scenario}"
                        );
                        for run in history
                            .page
                            .runs
                            .iter()
                            .filter(|run| run.status == Status::Failed)
                        {
                            assert!(restored.insert(run.turn), "{scenario}");
                            assert_eq!(
                                run.input.as_ref().unwrap().message.text,
                                input,
                                "{scenario}"
                            );
                        }
                        let Some(next) = history.page.next_before else {
                            break;
                        };
                        assert_ne!(before, Some(next), "{scenario}");
                        before = Some(next);
                    }
                    assert_eq!(restored, expected.into_iter().collect(), "{scenario}");
                })
                .await
            }
            "defers_turns_above_a_partial_turn" => {
                Box::pin(async {
                    let first = fixture.completed(&["first answer"]).await;
                    let second = fixture.completed(&["second answer"]).await;
                    let large = fixture.completed(&vec!["large event 中文"; 150]).await;
                    let recent = fixture.read(None, 20).await;
                    // Turns older than the partial turn return through the next page instead of
                    // appearing empty above it.
                    assert_eq!(
                        recent
                            .page
                            .runs
                            .iter()
                            .map(|run| run.turn)
                            .collect::<Vec<_>>(),
                        [large],
                        "{scenario}"
                    );
                    assert_eq!(recent.missing, [large], "{scenario}");
                    assert_eq!(recent.page.entries.len(), 100, "{scenario}");
                    assert_eq!(recent.page.next_before, Some(large), "{scenario}");
                    let older = fixture.read(recent.page.next_before, 20).await;
                    assert_eq!(
                        older
                            .page
                            .runs
                            .iter()
                            .map(|run| run.turn)
                            .collect::<Vec<_>>(),
                        [first, second],
                        "{scenario}"
                    );
                    assert!(older.missing.is_empty(), "{scenario}");
                    assert_eq!(older.page.next_before, None, "{scenario}");
                    // Continuations may exceed the logical page limit within the frame budget.
                    let chunk = turn(
                        &fixture.database.connection,
                        fixture.session,
                        large,
                        Some(recent.page.entries[0].sequence),
                        500,
                    )
                    .await
                    .unwrap();
                    assert_eq!(chunk.entries.len(), 50, "{scenario}");
                    assert_eq!(chunk.next_before, None, "{scenario}");
                    let whole = turn(
                        &fixture.database.connection,
                        fixture.session,
                        large,
                        None,
                        500,
                    )
                    .await
                    .unwrap();
                    assert_eq!(whole.entries.len(), 150, "{scenario}");
                    assert_eq!(whole.entries[50..], recent.page.entries[..], "{scenario}");
                })
                .await
            }
            "seeks_turn_events_without_sorting" => {
                Box::pin(async {
                    let turn = fixture.completed(&["first", "second"]).await;
                    for (sql, parameters) in [
                        (
                            first_query(),
                            vec![
                                turso::Value::Text(fixture.session.to_string()),
                                turso::Value::Text(turn.to_string()),
                            ],
                        ),
                        (
                            turn_query(),
                            vec![
                                turso::Value::Text(fixture.session.to_string()),
                                turso::Value::Text(turn.to_string()),
                                turso::Value::Integer(i64::MAX),
                                turso::Value::Integer(10),
                            ],
                        ),
                    ] {
                        let mut rows = fixture
                            .database
                            .connection
                            .query(format!("EXPLAIN QUERY PLAN {sql}"), parameters)
                            .await
                            .unwrap();
                        let mut plan = Vec::new();
                        while let Some(row) = rows.next().await.unwrap() {
                            plan.push(row.get::<String>(3).unwrap());
                        }
                        assert!(
                            plan.iter().any(|line| line.contains("SEARCH e ")
                                && line.contains("agent_events_turn")
                                && line.contains("turn=?")),
                            "{scenario}: {plan:?}"
                        );
                        assert!(
                        !plan
                            .iter()
                            .any(|line| line.contains("SORTER") || line.contains("TEMP B-TREE")),
            "{scenario}: {plan:?}"
                    );
                        assert!(
                            plan.iter().any(|line| line.contains("SEARCH h ")
                                && line.contains("session=? AND sequence=?")),
                            "{scenario}: {plan:?}"
                        );
                    }
                })
                .await
            }
            _ => unreachable!(),
        }
    }
}
