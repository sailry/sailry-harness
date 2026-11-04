use super::*;

fn run(
    base: &Run,
    sequence: u64,
    status: Status,
    started: Option<i64>,
    finished: Option<i64>,
) -> Run {
    Run {
        turn: TurnId::new(),
        sequence,
        status,
        started_ms: started,
        finished_ms: finished,
        ..base.clone()
    }
}

fn order(snapshot: &Snapshot) -> Vec<TurnId> {
    super::super::super::turns::timeline(&snapshot.page)
}

#[test]
fn preserves_admission_and_execution_order() {
    for (case_name, run) in [
        (
            "sends_reordered_input_at_its_execution_position",
            (|case_name| {
                let (_, mut snapshot, base, _) = fixture();
                // B was admitted after A but moved ahead and started first.
                let a = run(&base, 2, Status::Running, Some(3000), None);
                let b = run(&base, 3, Status::Completed, Some(2000), Some(2900));
                let first = run(&base, 1, Status::Completed, Some(1000), Some(1900));
                Arc::make_mut(&mut snapshot.page).runs = vec![first.clone(), a.clone(), b.clone()];
                assert_eq!(
                    order(&snapshot),
                    vec![first.turn, b.turn, a.turn],
                    "{case_name}"
                );
                // Admission order remains the page order that rewind and pagination use.
                let page: Vec<_> = snapshot.page.runs.iter().map(|run| run.turn).collect();
                assert_eq!(page, vec![first.turn, a.turn, b.turn], "{case_name}");
            }) as fn(&str),
        ),
        (
            "keeps_stopped_turn_before_later_turns",
            (|case_name| {
                let (_, mut snapshot, base, _) = fixture();
                let stopped = run(&base, 1, Status::Cancelled, Some(1000), Some(1200));
                let later = run(&base, 2, Status::Completed, Some(1500), Some(1800));
                let waiting = run(&base, 3, Status::Queued, None, None);
                let removed = run(&base, 4, Status::Cancelled, None, Some(1300));
                Arc::make_mut(&mut snapshot.page).runs = vec![
                    stopped.clone(),
                    later.clone(),
                    waiting.clone(),
                    removed.clone(),
                ];
                // Unstarted input leaves the queue at its removal time; waiting input follows.
                assert_eq!(
                    order(&snapshot),
                    vec![stopped.turn, removed.turn, later.turn, waiting.turn],
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "snapshot_keeps_admission_order",
            (|case_name| {
                let (mut projection, mut snapshot, base, _) = fixture();
                let a = run(&base, 1, Status::Completed, Some(2000), Some(2100));
                let b = run(&base, 2, Status::Completed, Some(1000), Some(1100));
                Arc::make_mut(&mut snapshot.page).runs = vec![a.clone(), b.clone()];
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot)))
                    .unwrap();
                let snapshot = projection.snapshot().unwrap();
                let page: Vec<_> = snapshot.page.runs.iter().map(|run| run.turn).collect();
                assert_eq!(page, vec![a.turn, b.turn], "{case_name}");
                assert_eq!(order(snapshot), vec![b.turn, a.turn], "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
