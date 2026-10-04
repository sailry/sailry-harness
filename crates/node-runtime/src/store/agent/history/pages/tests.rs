use super::*;
use crate::store::agent::tests::{command, fixture::Fixture};

#[test]
fn preserves_turn_boundaries() {
    let mut fixture = Fixture::new();
    let first = fixture.completed(&["first prompt", "first call", "first result", "first answer"]);
    let second = fixture.completed(&["second prompt", "second answer"]);
    let third = fixture.completed(&["third prompt", "third answer"]);
    let queued = fixture.queued();
    let cancelled = fixture.queued();
    command(
        &mut fixture.database,
        &fixture.events,
        Command::RemoveQueuedTurn {
            turn: cancelled,
            expected_revision: 1,
        },
    );
    let recent = fixture.read(None, 1);
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
    let older = fixture.read(recent.page.next_before, 1);
    assert_eq!(older.page.next_before, Some(second));
    assert_eq!(older.page.runs.len(), 1);
    assert!(older.page.entries.iter().all(|entry| entry.turn == second));
    let oldest = fixture.read(older.page.next_before, 1);
    assert_eq!(oldest.page.next_before, None);
    assert_eq!(oldest.page.entries.len(), 4);
    assert!(oldest.page.entries.iter().all(|entry| entry.turn == first));
    let combined = fixture.read(Some(third), 2);
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

#[test]
fn chunks_complete_turns() {
    let mut fixture = Fixture::new();
    let text = "中文🙂".repeat(90_000);
    let large = fixture.completed(&[text.as_str(); 7]);
    let recent = fixture.read(None, 1);
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

    let many = fixture.completed(&vec!["small event"; 105]);
    let recent = fixture.read(None, 1);
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
    .unwrap();
    assert_eq!(chunk.entries.len(), 5);
    assert_eq!(chunk.next_before, None);
}

#[test]
fn validates_resource_boundaries_and_limits() {
    let mut fixture = Fixture::new();
    let first = fixture.completed(&[]);
    let original = fixture.read(None, 1);
    assert_eq!(original.page.runs[0].turn, first);
    assert!(original.page.entries.is_empty());
    let other = SessionId::new();
    assert_eq!(
        turn(&fixture.database.connection, other, first, None, 1)
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
        .is_err()
    );
    for limit in [0, 101] {
        assert!(read(&fixture.database.connection, fixture.session, None, limit).is_err());
        assert!(
            turn(
                &fixture.database.connection,
                fixture.session,
                first,
                None,
                limit
            )
            .is_err()
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
        .is_err()
    );
    assert_eq!(fixture.read(None, 1), original);
}
