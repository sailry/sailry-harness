use super::*;
use sailry_protocol::conversation::TurnHistory;

fn entry(run: &Run, sequence: u64) -> Entry {
    Entry {
        sequence,
        id: format!("entry-{sequence}"),
        turn: run.turn,
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 1,
        parts: vec![Part::Text(format!("答复 {sequence}"))],
        citations: Vec::new(),
        search_suggestions: None,
        usage: None,
    }
}

fn ids(projection: &Projection) -> Vec<u64> {
    projection
        .snapshot()
        .unwrap()
        .page
        .entries
        .iter()
        .map(|entry| entry.sequence)
        .collect()
}

fn chunk(snapshot: &Snapshot, run: &Run, entries: Vec<Entry>) -> TurnHistory {
    TurnHistory {
        sequence: snapshot.sequence,
        revision: snapshot.page.revision,
        run: run.clone(),
        entries,
        approvals: Vec::new(),
        questions: Vec::new(),
        children: Vec::new(),
        next_before: None,
    }
}

#[test]
fn recovers_ordered_visibility_and_partial_turns() {
    for (case_name, run) in [
        (
            "reveals_and_hides_in_order",
            (|case_name| {
                let (mut projection, mut snapshot, run, _) = fixture();
                Arc::make_mut(&mut snapshot.page).entries =
                    vec![entry(&run, 2), entry(&run, 4)].into();
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let applied = projection
                    .apply(1, frame(&snapshot, 11, Change::Revealed(entry(&run, 3))))
                    .unwrap();
                assert_eq!(applied, Apply::Applied, "{case_name}");
                assert_eq!(ids(&projection), [2, 3, 4], "{case_name}");
                assert_eq!(projection.live_start(), None, "{case_name}");
                projection
                    .apply(1, frame(&snapshot, 12, Change::Revealed(entry(&run, 5))))
                    .unwrap();
                assert_eq!(ids(&projection), [2, 3, 4, 5], "{case_name}");
                assert_eq!(projection.live_start(), Some(5), "{case_name}");
                projection
                    .apply(
                        1,
                        frame(
                            &snapshot,
                            13,
                            Change::Hidden {
                                turn: run.turn,
                                id: "entry-3".into(),
                            },
                        ),
                    )
                    .unwrap();
                assert_eq!(ids(&projection), [2, 4, 5], "{case_name}");
                assert_eq!(projection.snapshot().unwrap().sequence, 13, "{case_name}");
                // A changed identity is still rejected rather than silently replaced.
                let mut changed = entry(&run, 4);
                changed.parts = vec![Part::Text("changed".into())];
                assert!(
                    projection
                        .apply(1, frame(&snapshot, 14, Change::Revealed(changed)))
                        .is_err(),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "defers_reveals_before_loaded_range",
            (|case_name| {
                let (mut projection, mut snapshot, run, _) = fixture();
                Arc::make_mut(&mut snapshot.page).entries =
                    vec![entry(&run, 5), entry(&run, 6)].into();
                snapshot.missing = vec![run.turn];
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                // Read before both changes; neither visibility change is in the continuation yet.
                let stale = chunk(
                    &snapshot,
                    &run,
                    vec![entry(&run, 1), entry(&run, 3), entry(&run, 4)],
                );
                projection
                    .apply(1, frame(&snapshot, 11, Change::Revealed(entry(&run, 2))))
                    .unwrap();
                projection
                    .apply(
                        1,
                        frame(
                            &snapshot,
                            12,
                            Change::Hidden {
                                turn: run.turn,
                                id: "entry-4".into(),
                            },
                        ),
                    )
                    .unwrap();
                // Inserting before the loaded range would leave a hole the next cursor skips.
                assert_eq!(ids(&projection), [5, 6], "{case_name}");
                assert_eq!(
                    projection.merge_turn(1, stale).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(ids(&projection), [1, 2, 3, 5, 6], "{case_name}");
                assert!(
                    projection.snapshot().unwrap().missing.is_empty(),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "continues_partial_turns",
            (|case_name| {
                let (mut projection, mut snapshot, run, _) = fixture();
                Arc::make_mut(&mut snapshot.page).entries = vec![entry(&run, 5)].into();
                snapshot.missing = vec![run.turn];
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let mut partial = chunk(&snapshot, &run, vec![entry(&run, 3), entry(&run, 4)]);
                partial.next_before = Some(3);
                assert_eq!(
                    projection.merge_turn(1, partial).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(ids(&projection), [3, 4, 5], "{case_name}");
                assert_eq!(
                    projection.snapshot().unwrap().missing,
                    [run.turn],
                    "{case_name}"
                );
                // A continuation newer than the stream waits for the stream to catch up.
                let mut early = chunk(&snapshot, &run, vec![entry(&run, 1)]);
                early.sequence = snapshot.sequence + 1;
                assert_eq!(
                    projection.merge_turn(1, early).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                let last = chunk(&snapshot, &run, vec![entry(&run, 1)]);
                assert_eq!(
                    projection.merge_turn(1, last.clone()).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(ids(&projection), [1, 3, 4, 5], "{case_name}");
                assert!(
                    projection.snapshot().unwrap().missing.is_empty(),
                    "{case_name}"
                );
                // A completed turn ignores duplicate continuations.
                assert_eq!(
                    projection.merge_turn(1, last).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                let mut moved = chunk(&snapshot, &run, Vec::new());
                moved.revision += 1;
                assert_eq!(
                    projection.merge_turn(2, moved.clone()).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                snapshot.missing = vec![run.turn];
                projection.reconnect(2).unwrap();
                projection
                    .apply(2, Update::ConversationSnapshot(Box::new(snapshot)))
                    .unwrap();
                assert_eq!(
                    projection.merge_turn(2, moved).unwrap(),
                    Apply::Recover,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
