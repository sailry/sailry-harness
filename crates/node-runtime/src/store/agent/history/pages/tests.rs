use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

#[tokio::test]
async fn fits_cached_entry_lengths_exactly() {
    let mut fixture = Fixture::new().await;
    let turn = fixture.completed(&["escaped \"\n", "中文🙂", "tail"]).await;
    let mut page = fixture.read(None, 1).await.page;
    let entries = page.entries.clone();
    let bytes = entries
        .iter()
        .map(|entry| encode(entry).unwrap().len())
        .sum::<usize>();
    assert_eq!(
        encoded_size(&mut page, bytes).unwrap(),
        encode(&page).unwrap().len()
    );
    let budget = encode(&page).unwrap().len() - encode(&entries[0]).unwrap().len() - 1;
    let headroom = MAX_FRAME_BYTES - 64 * 1024 - budget;
    assert_eq!(
        fit(&mut page, headroom, true).unwrap(),
        BTreeSet::from([turn])
    );
    assert_eq!(page.entries, entries[1..]);
    assert_eq!(encode(&page).unwrap().len(), budget);
    page.entries.clear();
    assert_eq!(
        encoded_size(&mut page, 0).unwrap(),
        encode(&page).unwrap().len()
    );
}

#[tokio::test]
async fn preserves_turn_boundaries() {
    let mut fixture = Fixture::new().await;
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
    assert!(recent.missing.is_empty());
    assert_eq!(recent.page.next_before, Some(third));
    assert_eq!(
        recent
            .page
            .runs
            .iter()
            .map(|run| run.turn)
            .collect::<Vec<_>>(),
        [third, queued, cancelled]
    );
    assert_eq!(recent.page.entries.len(), 2);
    assert!(recent.page.entries.iter().all(|entry| entry.turn == third));
    assert_eq!(recent.page.queue.items.len(), 1);
    assert_eq!(recent.page.queue.items[0].turn, queued);
    let older = fixture.read(recent.page.next_before, 1).await;
    assert_eq!(older.page.next_before, Some(second));
    assert_eq!(older.page.runs.len(), 1);
    assert!(older.page.entries.iter().all(|entry| entry.turn == second));
    let oldest = fixture.read(older.page.next_before, 1).await;
    assert_eq!(oldest.page.next_before, None);
    assert_eq!(oldest.page.entries.len(), 4);
    assert!(oldest.page.entries.iter().all(|entry| entry.turn == first));
    let combined = fixture.read(Some(third), 2).await;
    assert_eq!(combined.page.entries.len(), 6);
    assert_eq!(
        combined
            .page
            .runs
            .iter()
            .map(|run| run.turn)
            .collect::<Vec<_>>(),
        [first, second]
    );
    assert_eq!(combined.page.next_before, None);
}

#[tokio::test]
async fn chunks_complete_turns() {
    let mut fixture = Fixture::new().await;
    let text = "中文🙂".repeat(90_000);
    let large = fixture.completed(&[text.as_str(); 7]).await;
    let recent = fixture.read(None, 1).await;
    assert_eq!(recent.page.next_before, None);
    assert_eq!(recent.missing, [large]);
    assert!(encode(&Output::Conversation(recent.clone())).unwrap().len() < MAX_FRAME_BYTES);
    let mut restored = recent.page.entries;
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
        assert!(!chunk.entries.is_empty());
        assert!(
            chunk
                .entries
                .iter()
                .all(|entry| entry.turn == large && entry.sequence < before)
        );
        assert!(encode(&Output::TurnHistory(chunk.clone())).unwrap().len() < MAX_FRAME_BYTES);
        let next = chunk.next_before;
        let mut entries = chunk.entries;
        entries.append(&mut restored);
        restored = entries;
        chunks += 1;
        if next.is_none() {
            break;
        }
        assert_eq!(next, Some(restored[0].sequence));
        assert!(chunks < 7);
    }
    assert_eq!(chunks, 2);
    assert_eq!(restored.len(), 7);
    assert!(
        restored
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    );
    assert!(
        restored
            .iter()
            .all(|entry| entry.parts == [Part::Text(text.clone())])
    );

    let many = fixture.completed(&vec!["small event"; 105]).await;
    let recent = fixture.read(None, 1).await;
    assert_eq!(recent.page.runs[0].turn, many);
    assert_eq!(recent.missing, [many]);
    assert_eq!(recent.page.entries.len(), 100);
    assert_eq!(recent.page.next_before, Some(many));
    let chunk = turn(
        &fixture.database.connection,
        fixture.session,
        many,
        Some(recent.page.entries[0].sequence),
        100,
    )
    .await
    .unwrap();
    assert_eq!(chunk.entries.len(), 5);
    assert_eq!(chunk.next_before, None);
}

#[tokio::test]
async fn validates_resource_boundaries_and_limits() {
    let mut fixture = Fixture::new().await;
    let first = fixture.completed(&[]).await;
    let original = fixture.read(None, 1).await;
    assert_eq!(original.page.runs[0].turn, first);
    assert!(original.page.entries.is_empty());
    let other = SessionId::new();
    assert_eq!(
        turn(&fixture.database.connection, other, first, None, 1)
            .await
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
    assert!(
        read(
            &fixture.database.connection,
            fixture.session,
            Some(TurnId::new()),
            1
        )
        .await
        .is_err()
    );
    assert!(
        read(&fixture.database.connection, fixture.session, None, 0)
            .await
            .is_err()
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
        .is_err()
    );
    for limit in [101, u16::MAX] {
        assert_eq!(
            read(&fixture.database.connection, fixture.session, None, limit)
                .await
                .unwrap(),
            original
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
            .is_empty()
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
        .is_err()
    );
    assert_eq!(fixture.read(None, 1).await, original);
}

#[tokio::test]
async fn bounds_failed_inputs_without_truncation() {
    let mut fixture = Fixture::new().await;
    // JSON expands these source bytes sixfold; the logical page cannot fit all captures.
    let input = format!("Task {}", "\u{0001}".repeat(1024 * 1024 - 5));
    let mut expected = Vec::new();
    for _ in 0..5 {
        let turn = fixture.queued().await;
        // Each failed run pauses dispatch; explicitly resume the next fixture input.
        super::super::queue::pause(&fixture.database.connection, fixture.session, false)
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
            turn
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
                < MAX_FRAME_BYTES
        );
        assert!(history.page.entries.is_empty());
        assert!(history.missing.is_empty());
        assert!(
            history
                .page
                .queue
                .items
                .iter()
                .any(|item| item.turn == queued)
        );
        for run in history
            .page
            .runs
            .iter()
            .filter(|run| run.status == Status::Failed)
        {
            assert!(restored.insert(run.turn));
            assert_eq!(run.input.as_ref().unwrap().message.text, input);
        }
        let Some(next) = history.page.next_before else {
            break;
        };
        assert_ne!(before, Some(next));
        before = Some(next);
    }
    assert_eq!(restored, expected.into_iter().collect());
}
