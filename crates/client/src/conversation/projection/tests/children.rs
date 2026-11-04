use super::*;
use conversation::{Child, History};

fn setup() -> (Projection, Snapshot, Child) {
    let (projection, mut snapshot, run, _) = fixture();
    let origin = Delegation {
        session: run.session,
        turn: run.turn,
        entry: "parent-call".into(),
        index: 0,
        role: Some(RoleId::new()),
    };
    Arc::make_mut(&mut Arc::make_mut(&mut snapshot.page).entries).push(Entry {
        sequence: 1,
        id: origin.entry.clone(),
        turn: run.turn,
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 1,
        usage: None,
        citations: vec![],
        search_suggestions: None,
        parts: vec![Part::ToolCall {
            presentation: Default::default(),
            grouping: Default::default(),
            id: Some("call".into()),
            name: "third_party_delegate".into(),
            summary: Default::default(),
        }],
    });
    let child = Child {
        run: Run {
            worktree: run.worktree,
            session: SessionId::new(),
            turn: TurnId::new(),
            sequence: 2,
            revision: 1,
            ..run
        },
        origin,
        name: Some("Frozen review".into()),
    };
    (projection, snapshot, child)
}

#[test]
fn recovers_ordered_children_and_history() {
    for (case_name, run) in [
        (
            "orders_updates_and_recovers_gaps",
            (|case_name| {
                let (mut projection, snapshot, mut child) = setup();
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let started = frame(&snapshot, 11, Change::Child(child.clone()));
                assert_eq!(
                    projection.apply(1, started.clone()).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(
                    projection.apply(1, started).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                child.run.status = Status::Stopping;
                projection
                    .apply(1, frame(&snapshot, 12, Change::Child(child.clone())))
                    .unwrap();
                child.run.status = Status::Running;
                let stable = projection.snapshot().unwrap().clone();
                assert!(
                    projection
                        .apply(1, frame(&snapshot, 13, Change::Child(child.clone())))
                        .is_err(),
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap(), &stable, "{case_name}");
                child.run.status = Status::Cancelled;
                assert_eq!(
                    projection
                        .apply(1, frame(&snapshot, 14, Change::Child(child.clone())))
                        .unwrap(),
                    Apply::Recover,
                    "{case_name}"
                );
                projection.reconnect(2).unwrap();
                assert_eq!(
                    projection
                        .apply(1, frame(&snapshot, 13, Change::Child(child.clone())))
                        .unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                let mut recovered = snapshot.clone();
                recovered.sequence = 0;
                Arc::make_mut(&mut recovered.page).children = vec![child.clone()];
                projection
                    .apply(2, Update::ConversationSnapshot(Box::new(recovered.clone())))
                    .unwrap();
                child.run.status = Status::Completed;
                assert!(
                    projection
                        .apply(2, frame(&snapshot, 1, Change::Child(child)))
                        .is_err(),
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap(), &recovered, "{case_name}");
            }) as fn(&str),
        ),
        (
            "preserves_live_state_on_merge",
            (|case_name| {
                let (mut projection, mut snapshot, mut child) = setup();
                Arc::make_mut(&mut snapshot.page)
                    .children
                    .push(child.clone());
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                child.run.status = Status::Completed;
                projection
                    .apply(1, frame(&snapshot, 11, Change::Child(child.clone())))
                    .unwrap();
                let old = History {
                    sequence: 10,
                    page: (*snapshot.page).clone(),
                    missing: vec![],
                };
                assert_eq!(
                    projection.merge_history(1, None, old.clone()).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().page.children,
                    [child.clone()],
                    "{case_name}"
                );
                let mut conflicting = old;
                conflicting.page.children[0].run.status = Status::Failed;
                assert!(
                    projection.merge_history(1, None, conflicting).is_err(),
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().page.children,
                    [child],
                    "{case_name}"
                );

                let branch = SessionId::new();
                let inherited = Arc::make_mut(&mut snapshot.page);
                inherited.session = branch;
                inherited.runs[0].origin = Some(inherited.runs[0].session);
                inherited.runs[0].session = branch;
                inherited.runs[0].status = Status::Completed;
                inherited.children[0].run.status = Status::Completed;
                let mut projection = Projection::new(snapshot.node, branch, 1);
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                assert_ne!(
                    snapshot.page.children[0].origin.session, branch,
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap(), &snapshot, "{case_name}");
            }) as fn(&str),
        ),
        (
            "hydrates_from_earlier_call",
            (|case_name| {
                let (_, snapshot, child) = setup();
                let mut page = (*snapshot.page).clone();
                std::sync::Arc::make_mut(&mut page.entries).clear();
                let mut chunk = (*snapshot.page).clone();
                chunk.children.push(child.clone());
                super::super::super::history::merge(&mut page, chunk.clone(), false).unwrap();
                assert_eq!(page.children, [child], "{case_name}");
                assert_eq!(
                    page.entries.as_slice(),
                    chunk.entries.as_slice(),
                    "{case_name}"
                );
                super::super::super::history::merge(&mut page, chunk.clone(), false).unwrap();
                assert_eq!(page, chunk, "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn rejects_forged_child_ownership() {
    for (case_name, run) in [
        (
            "rejects_forged_identity",
            (|case_name| {
                let (mut projection, mut snapshot, child) = setup();
                Arc::make_mut(&mut snapshot.page)
                    .children
                    .push(child.clone());
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let mut variants = vec![];
                let mut forged = child.clone();
                forged.run.session = SessionId::new();
                variants.push(forged);
                let mut forged = child.clone();
                forged.run.turn = TurnId::new();
                variants.push(forged);
                let mut forged = child.clone();
                forged.run.sequence += 1;
                variants.push(forged);
                let mut forged = child.clone();
                forged.run.revision += 1;
                variants.push(forged);
                let mut forged = child.clone();
                forged.run.kind = conversation::RunKind::Compaction;
                variants.push(forged);
                let mut forged = child.clone();
                forged.origin.session = SessionId::new();
                variants.push(forged);
                let mut forged = child.clone();
                forged.origin.index = 1;
                variants.push(forged);
                let mut forged = child.clone();
                forged.name = Some("Changed".into());
                variants.push(forged);
                for forged in variants {
                    assert!(
                        projection
                            .apply(1, frame(&snapshot, 11, Change::Child(forged)))
                            .is_err(),
                        "{case_name}"
                    );
                    assert_eq!(projection.snapshot().unwrap(), &snapshot, "{case_name}");
                }
                let mut repeated = snapshot.clone();
                Arc::make_mut(&mut repeated.page).children.push(child);
                assert!(
                    projection
                        .apply(1, Update::ConversationSnapshot(Box::new(repeated)))
                        .is_err(),
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap(), &snapshot, "{case_name}");
            }) as fn(&str),
        ),
        (
            "validates_worktree_selection",
            (|case_name| {
                let (mut projection, mut snapshot, mut child) = setup();
                let target = WorktreeId::new();
                let page = Arc::make_mut(&mut snapshot.page);
                let Part::ToolCall { summary, .. } =
                    &mut std::sync::Arc::make_mut(&mut page.entries)
                        .last_mut()
                        .unwrap()
                        .parts[0]
                else {
                    panic!("delegation call expected")
                };
                summary.delegation.worktree = Some(target);
                child.run.worktree = target;
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                projection
                    .apply(1, frame(&snapshot, 11, Change::Child(child.clone())))
                    .unwrap();
                let stable = projection.snapshot().unwrap().clone();
                child.run.worktree = WorktreeId::new();
                assert!(
                    projection
                        .apply(1, frame(&snapshot, 12, Change::Child(child)))
                        .is_err(),
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap(), &stable, "{case_name}");
            }) as fn(&str),
        ),
        (
            "rejects_parent_relocation",
            (|case_name| {
                let (mut projection, snapshot, _) = setup();
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let mut run = snapshot.page.runs[0].clone();
                run.worktree = WorktreeId::new();
                assert!(
                    projection
                        .apply(1, frame(&snapshot, 11, Change::Run(run.clone())))
                        .is_err(),
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().page,
                    snapshot.page,
                    "{case_name}"
                );
                let mut page = (*snapshot.page).clone();
                let mut incoming = page.clone();
                incoming.runs[0] = run;
                assert!(
                    crate::conversation::history::merge(&mut page, incoming, true).is_err(),
                    "{case_name}"
                );
                assert_eq!(page, *snapshot.page, "{case_name}");
            }) as fn(&str),
        ),
        (
            "rejects_changed_reuse_ownership",
            (|case_name| {
                let (mut projection, mut snapshot, child) = setup();
                Arc::make_mut(&mut snapshot.page)
                    .children
                    .push(child.clone());
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let mut next = child.clone();
                next.run.turn = TurnId::new();
                next.run.sequence += 1;
                let mut variants = vec![];
                let mut forged = next.clone();
                forged.origin.role = Some(RoleId::new());
                variants.push(forged);
                let mut forged = next.clone();
                forged.name = Some("Changed role".into());
                variants.push(forged);
                let mut forged = next.clone();
                forged.run.revision += 1;
                variants.push(forged);
                let mut forged = next;
                forged.run.worktree = WorktreeId::new();
                variants.push(forged);
                for forged in variants {
                    assert!(
                        projection
                            .apply(1, frame(&snapshot, 11, Change::Child(forged)))
                            .is_err(),
                        "{case_name}"
                    );
                    assert_eq!(projection.snapshot().unwrap(), &snapshot, "{case_name}");
                }
                for selection in [
                    sailry_protocol::tool::detail::Delegation {
                        invalid: true,
                        ..Default::default()
                    },
                    sailry_protocol::tool::detail::Delegation {
                        session: Some(SessionId::new()),
                        ..Default::default()
                    },
                ] {
                    let mut forged = snapshot.clone();
                    let Part::ToolCall { summary, .. } =
                        &mut Arc::make_mut(&mut Arc::make_mut(&mut forged.page).entries)[0].parts
                            [0]
                    else {
                        panic!("delegation call expected")
                    };
                    summary.delegation = selection;
                    assert!(
                        projection
                            .apply(1, Update::ConversationSnapshot(Box::new(forged)))
                            .is_err(),
                        "{case_name}"
                    );
                    assert_eq!(projection.snapshot().unwrap(), &snapshot, "{case_name}");
                }
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn preserves_reused_and_guided_history() {
    for (case_name, run) in [
        (
            "preserves_reused_and_guided_runs",
            (|case_name| {
                let (mut projection, mut snapshot, mut first) = setup();
                first.run.status = Status::Completed;
                let mut parent = snapshot.page.runs[0].clone();
                parent.turn = TurnId::new();
                parent.sequence = 3;
                let mut call = snapshot.page.entries[0].clone();
                call.id = "reuse-call".into();
                call.sequence = 2;
                call.turn = parent.turn;
                let Part::ToolCall { summary, .. } = &mut call.parts[0] else {
                    panic!("delegation call expected")
                };
                summary.delegation.session = Some(first.run.session);
                let page = Arc::make_mut(&mut snapshot.page);
                page.runs[0].status = Status::Completed;
                page.runs.push(parent.clone());
                std::sync::Arc::make_mut(&mut page.entries).push(call.clone());
                page.children.push(first.clone());
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let mut reused = first.clone();
                reused.run.turn = TurnId::new();
                reused.run.sequence = 4;
                reused.run.status = Status::Running;
                reused.origin.turn = parent.turn;
                reused.origin.entry = call.id;
                projection
                    .apply(1, frame(&snapshot, 11, Change::Child(reused.clone())))
                    .unwrap();
                let mut guided = reused.clone();
                guided.run.turn = TurnId::new();
                guided.run.sequence = 5;
                guided.run.status = Status::Queued;
                projection
                    .apply(1, frame(&snapshot, 12, Change::Child(guided.clone())))
                    .unwrap();
                reused.run.status = Status::Cancelled;
                projection
                    .apply(1, frame(&snapshot, 13, Change::Child(reused.clone())))
                    .unwrap();
                guided.run.status = Status::Running;
                projection
                    .apply(1, frame(&snapshot, 14, Change::Child(guided.clone())))
                    .unwrap();
                let expected = vec![first.clone(), reused.clone(), guided.clone()];
                assert_eq!(
                    projection.snapshot().unwrap().page.children,
                    expected,
                    "{case_name}"
                );
                let older = History {
                    sequence: snapshot.sequence,
                    page: (*snapshot.page).clone(),
                    missing: vec![],
                };
                projection.merge_history(1, None, older).unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().page.children,
                    expected,
                    "{case_name}"
                );
                let mut recovered = projection.snapshot().unwrap().clone();
                recovered.sequence = 0;
                projection.reconnect(2).unwrap();
                projection
                    .apply(2, Update::ConversationSnapshot(Box::new(recovered)))
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().page.children,
                    expected,
                    "{case_name}"
                );
                assert_ne!(first.origin.turn, reused.origin.turn, "{case_name}");
                assert_eq!(reused.origin, guided.origin, "{case_name}");
            }) as fn(&str),
        ),
        (
            "hydrates_reuse_without_founding_call",
            (|case_name| {
                let (_, mut snapshot, mut child) = setup();
                child.run.worktree = WorktreeId::new();
                let Part::ToolCall { summary, .. } =
                    &mut Arc::make_mut(&mut Arc::make_mut(&mut snapshot.page).entries)[0].parts[0]
                else {
                    panic!("delegation call expected")
                };
                summary.delegation.session = Some(child.run.session);
                Arc::make_mut(&mut snapshot.page)
                    .children
                    .push(child.clone());
                let mut page = (*snapshot.page).clone();
                std::sync::Arc::make_mut(&mut page.entries).clear();
                page.children.clear();
                let chunk = (*snapshot.page).clone();
                crate::conversation::history::merge(&mut page, chunk.clone(), false).unwrap();
                assert_eq!(page, chunk, "{case_name}");
                assert_eq!(page.children, [child], "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
