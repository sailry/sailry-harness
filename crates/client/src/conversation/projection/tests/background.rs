use super::*;
use sailry_protocol::conversation::background::{Kind, Outcome, State, Task};

fn task(snapshot: &Snapshot, run: &Run) -> Task {
    let id = RequestId::new();
    Task {
        id: format!("command:{id}"),
        session: snapshot.page.session,
        turn: run.turn,
        kind: Kind::Command { id },
        title: "Fixture command".into(),
        revision: 1,
        state: State::Running,
        outcome: None,
        started_ms: 1000,
        finished_ms: None,
        summary: None,
        delivery: None,
        recovered: false,
        suppressed: false,
    }
}

#[test]
fn recovers_revisions_and_validates_captured_identity() {
    for (case_name, run) in [
        (
            "follows_revisions_through_recovery",
            (|case_name| {
                let (mut projection, snapshot, run, _) = fixture();
                let mut task = task(&snapshot, &run);
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                projection
                    .apply(1, frame(&snapshot, 11, Change::Background(task.clone())))
                    .unwrap();
                task.revision = 2;
                task.state = State::Pending;
                task.outcome = Some(Outcome::Completed);
                task.finished_ms = Some(2000);
                let older = task.clone();
                projection
                    .apply(1, frame(&snapshot, 12, Change::Background(task.clone())))
                    .unwrap();
                task.revision = 3;
                task.state = State::Delivered;
                task.delivery = Some(TurnId::new());
                projection
                    .apply(1, frame(&snapshot, 13, Change::Background(task.clone())))
                    .unwrap();
                task.revision = 4;
                task.state = State::Pending;
                task.delivery = None;
                task.recovered = true;
                projection
                    .apply(1, frame(&snapshot, 14, Change::Background(task.clone())))
                    .unwrap();
                projection
                    .apply(1, frame(&snapshot, 15, Change::Background(older)))
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().page.background,
                    [task],
                    "{case_name}"
                );
                let restored = projection.snapshot().unwrap().clone();
                projection.reconnect(2).unwrap();
                projection
                    .apply(2, Update::ConversationSnapshot(Box::new(restored.clone())))
                    .unwrap();
                assert_eq!(projection.snapshot().unwrap(), &restored, "{case_name}");
            }) as fn(&str),
        ),
        (
            "rejects_changed_ownership_and_unversioned_content",
            (|case_name| {
                let (mut projection, mut snapshot, run, _) = fixture();
                let original = task(&snapshot, &run);
                Arc::make_mut(&mut snapshot.page).background = vec![original.clone()];
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let mut foreign = original.clone();
                foreign.session = SessionId::new();
                foreign.revision += 1;
                let mut source = original.clone();
                source.turn = TurnId::new();
                source.revision += 1;
                let mut changed = original.clone();
                changed.title = "Unversioned".into();
                let mut wrong_id = original;
                wrong_id.id = "command:invalid".into();
                for task in [foreign, source, changed, wrong_id] {
                    assert!(
                        projection
                            .apply(1, frame(&snapshot, 11, Change::Background(task)))
                            .is_err(),
                        "{case_name}"
                    );
                    assert_eq!(
                        projection.snapshot().unwrap().page,
                        snapshot.page,
                        "{case_name}"
                    );
                }
            }) as fn(&str),
        ),
        (
            "accepts_stall_evidence_beside_running_command",
            (|case_name| {
                let (mut projection, snapshot, run, _) = fixture();
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let command = task(&snapshot, &run);
                let Kind::Command { id } = &command.kind else {
                    unreachable!()
                };
                let mut stalled = command.clone();
                stalled.id = format!("stall:{id}");
                stalled.state = State::Pending;
                stalled.outcome = Some(Outcome::Stalled);
                stalled.finished_ms = Some(2000);
                projection
                    .apply(1, frame(&snapshot, 11, Change::Background(command.clone())))
                    .unwrap();
                projection
                    .apply(1, frame(&snapshot, 12, Change::Background(stalled.clone())))
                    .unwrap();
                let mut foreign = stalled.clone();
                foreign.id = format!("stall:{}", RequestId::new());
                let mut finished = stalled.clone();
                finished.revision += 1;
                finished.outcome = Some(Outcome::Completed);
                for task in [foreign, finished] {
                    assert!(
                        projection
                            .apply(1, frame(&snapshot, 13, Change::Background(task)))
                            .is_err(),
                        "{case_name}"
                    );
                }
                assert_eq!(
                    projection.snapshot().unwrap().page.background,
                    [command, stalled],
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "keeps_child_identity_across_guidance_results",
            (|case_name| {
                let (mut projection, mut snapshot, run, _) = fixture();
                let mut source = task(&snapshot, &run);
                let child = SessionId::new();
                source.id = format!("agent:{}", TurnId::new());
                source.kind = Kind::Agent {
                    session: child,
                    turn: TurnId::new(),
                };
                Arc::make_mut(&mut snapshot.page)
                    .background
                    .push(source.clone());
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                source.revision += 1;
                source.kind = Kind::Agent {
                    session: child,
                    turn: TurnId::new(),
                };
                source.state = State::Pending;
                source.outcome = Some(Outcome::Completed);
                projection
                    .apply(1, frame(&snapshot, 11, Change::Background(source.clone())))
                    .unwrap();
                let mut forged = source.clone();
                forged.revision += 1;
                forged.kind = Kind::Agent {
                    session: SessionId::new(),
                    turn: TurnId::new(),
                };
                assert!(
                    projection
                        .apply(1, frame(&snapshot, 12, Change::Background(forged)))
                        .is_err(),
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().page.background,
                    [source],
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "notice_input_is_not_user_editable",
            (|case_name| {
                let (_, mut snapshot, mut run, _) = fixture();
                run.notice = true;
                let page = Arc::make_mut(&mut snapshot.page);
                page.runs = vec![run.clone()];
                std::sync::Arc::make_mut(&mut page.entries).push(Entry {
                    sequence: 1,
                    id: "system-input".into(),
                    turn: run.turn,
                    author: "user".into(),
                    branch: String::new(),
                    timestamp_ms: 1000,
                    parts: vec![Part::Text("Node context".into())],
                    citations: vec![],
                    search_suggestions: None,
                    usage: None,
                });
                assert!(
                    super::super::super::turns::input(page, run.turn).is_none(),
                    "{case_name}"
                );
                page.runs[0].notice = false;
                assert_eq!(
                    super::super::super::turns::input(page, run.turn)
                        .unwrap()
                        .message
                        .text,
                    "Node context",
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
