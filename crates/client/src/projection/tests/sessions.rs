use super::*;
use sailry_protocol::{conversation::question, *};

fn session() -> Session {
    Session {
        id: SessionId::new(),
        creation_order: 1,
        archived: false,
        activity: Default::default(),
        association: None,
        project: Some(ProjectId::new()),
        worktree: WorktreeId::new(),
        revision: 1,
        config: SessionConfig {
            features: Default::default(),
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
            options: Default::default(),
            permission: Permission::Ask,
            credential: None,
        },
        roles: Default::default(),

        profile: None,
        fork: None,
        delegation: None,
    }
}

#[test]
fn follows_settled_and_descendant_activity() {
    for (case_name, run) in [
        (
            "worktree_changes_follow_settled_activity",
            (|case_name| {
                let first = session();
                let second = session();
                let mut state = snapshot(0);
                state.sessions = vec![first.clone(), second.clone()];
                state.worktrees = [first.worktree, second.worktree]
                    .into_iter()
                    .map(|id| Worktree {
                        id,
                        project: first.project,
                        path: id.to_string(),
                        main: false,
                    })
                    .collect();
                let node = state.node;
                let mut projection = Projection::new(node, 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(state)))
                    .unwrap();
                let unchanged = Event::ConversationChanged {
                    session: first.id,
                    activity: Box::new(first.activity.clone()),
                };
                projection
                    .apply(
                        1,
                        Update::Event(EventEnvelope {
                            node,
                            cursor: 1,
                            event: unchanged,
                        }),
                    )
                    .unwrap();
                assert_eq!(
                    projection.settled_cursors()[&first.worktree],
                    0,
                    "{case_name}"
                );
                projection
                    .apply(
                        1,
                        Update::Event(EventEnvelope {
                            node,
                            cursor: 2,
                            event: Event::ToolsCompleted { session: first.id },
                        }),
                    )
                    .unwrap();
                assert_eq!(
                    projection.settled_cursors()[&first.worktree],
                    2,
                    "{case_name}"
                );
                assert_eq!(
                    projection.settled_cursors()[&second.worktree],
                    0,
                    "{case_name}"
                );
                projection
                    .apply(
                        1,
                        Update::Event(EventEnvelope {
                            node,
                            cursor: 3,
                            event: Event::ToolsCompleted { session: second.id },
                        }),
                    )
                    .unwrap();
                assert_eq!(
                    projection.settled_cursors()[&first.worktree],
                    2,
                    "{case_name}"
                );
                assert_eq!(
                    projection.settled_cursors()[&second.worktree],
                    3,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "descendant_activity_follows_events_and_recovery",
            (|case_name| {
                use crate::activity::{Lane, aggregate_lane};
                use conversation::{Run, RunKind, Status};
                let mut parent = session();
                parent.activity.run = Some(Run {
                    worktree: parent.worktree,
                    turn: TurnId::new(),
                    kind: RunKind::Task,
                    session: parent.id,
                    sequence: 1,
                    revision: 1,
                    status: Status::Completed,
                    error: None,
                    started_ms: None,
                    finished_ms: None,
                    origin: None,
                    continuation: None,
                    guidance: None,
                    joined: None,
                    notice: false,
                    notices: Vec::new(),
                    input: None,
                });
                let mut child = parent.clone();
                child.id = SessionId::new();
                child.delegation = Some(Box::new(Delegation {
                    session: parent.id,
                    turn: parent.activity.run.as_ref().unwrap().turn,
                    entry: "background-call".into(),
                    index: 0,
                    role: None,
                }));
                let run = child.activity.run.as_mut().unwrap();
                run.session = child.id;
                run.turn = TurnId::new();
                run.sequence = 2;
                run.status = Status::Queued;
                let mut state = snapshot(0);
                state.sessions = vec![parent.clone(), child.clone()];
                let mut projection = Projection::new(state.node, 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(state.clone())))
                    .unwrap();
                assert_eq!(
                    aggregate_lane(&parent, &projection.snapshot().unwrap().sessions),
                    Lane::Idle,
                    "{case_name}"
                );
                for (index, status) in [Status::Running, Status::Stopping, Status::Completed]
                    .into_iter()
                    .enumerate()
                {
                    child.activity.run.as_mut().unwrap().status = status;
                    let update = Update::Event(EventEnvelope {
                        node: state.node,
                        cursor: index as u64 + 1,
                        event: Event::ConversationChanged {
                            session: child.id,
                            activity: Box::new(child.activity.clone()),
                        },
                    });
                    assert_eq!(
                        projection.apply(1, update.clone()).unwrap(),
                        Apply::Applied,
                        "{case_name}"
                    );
                    assert_eq!(
                        projection.apply(1, update).unwrap(),
                        Apply::Ignored,
                        "{case_name}"
                    );
                    let snapshot = projection.snapshot().unwrap();
                    assert_eq!(
                        aggregate_lane(&parent, &snapshot.sessions),
                        if status == Status::Completed {
                            Lane::Idle
                        } else {
                            Lane::Running
                        },
                        "{case_name}"
                    );
                    assert_eq!(
                        snapshot.sessions[0].activity, parent.activity,
                        "{case_name}"
                    );
                }
                let gap = Update::Event(EventEnvelope {
                    node: state.node,
                    cursor: 5,
                    event: Event::ConversationChanged {
                        session: child.id,
                        activity: Box::new(child.activity.clone()),
                    },
                });
                assert_eq!(
                    projection.apply(1, gap).unwrap(),
                    Apply::Recover,
                    "{case_name}"
                );
                projection.reconnect(2).unwrap();
                state.cursor = 5;
                child.activity.run.as_mut().unwrap().status = Status::Running;
                state.sessions[1] = child;
                projection
                    .apply(2, Update::Snapshot(Box::new(state)))
                    .unwrap();
                assert_eq!(
                    aggregate_lane(&parent, &projection.snapshot().unwrap().sessions),
                    Lane::Running,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn recovers_new_workspaces_and_admitted_turns() {
    for (case_name, run) in [
        (
            "scheduled_session_recovers_workspace_and_keeps_turn",
            (|case_name| {
                for known in [false, true] {
                    let mut created = session();
                    created.project = None;
                    let mut state = snapshot(0);
                    if known {
                        state.worktrees.push(Worktree {
                            id: created.worktree,
                            project: None,
                            path: "/fixture/task".into(),
                            main: false,
                        });
                    }
                    let mut projection = Projection::new(state.node, 1);
                    projection
                        .apply(1, Update::Snapshot(Box::new(state)))
                        .unwrap();
                    let turn = QueuedTurn {
                        id: TurnId::new(),
                        kind: conversation::RunKind::Task,
                        session: created.id,
                        request: RequestId::new(),
                        revision: created.revision,
                        config: created.config.clone(),
                        roles: created.roles.clone(),
                        plugins: vec![],
                    };
                    let update = Update::Event(EventEnvelope {
                        node: NodeId([1; 32]),
                        cursor: 1,
                        event: Event::TaskStarted {
                            session: Box::new(created.clone()),
                            turn: turn.clone(),
                        },
                    });
                    assert_eq!(
                        projection.apply(1, update.clone()).unwrap(),
                        if known {
                            Apply::Applied
                        } else {
                            Apply::Recover
                        },
                        "{case_name}"
                    );
                    if known {
                        assert_eq!(
                            projection.snapshot().unwrap().sessions,
                            vec![created],
                            "{case_name}"
                        );
                        assert_eq!(
                            projection.snapshot().unwrap().turns,
                            vec![TurnSummary::from(&turn)],
                            "{case_name}"
                        );
                        assert_eq!(
                            projection.apply(1, update).unwrap(),
                            Apply::Ignored,
                            "{case_name}"
                        );
                    }
                }
            }) as fn(&str),
        ),
        (
            "new_unassigned_workspace_recovers_from_node",
            (|case_name| {
                let mut created = session();
                created.project = None;
                let mut state = snapshot(0);
                let mut projection = Projection::new(state.node, 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(state.clone())))
                    .unwrap();
                let event = Update::Event(EventEnvelope {
                    node: state.node,
                    cursor: 1,
                    event: Event::SessionChanged(Box::new(created.clone())),
                });
                assert_eq!(
                    projection.apply(1, event).unwrap(),
                    Apply::Recover,
                    "{case_name}"
                );
                state.cursor = 1;
                state.worktrees.push(Worktree {
                    id: created.worktree,
                    project: None,
                    path: "/fixture/sessions/workspace".into(),
                    main: false,
                });
                state.sessions.push(created.clone());
                projection
                    .apply(1, Update::Snapshot(Box::new(state)))
                    .unwrap();
                created.activity.title = "Updated".into();
                assert_ne!(
                    projection
                        .apply(
                            1,
                            Update::Event(EventEnvelope {
                                node: NodeId([1; 32]),
                                cursor: 2,
                                event: Event::SessionChanged(Box::new(created.clone())),
                            })
                        )
                        .unwrap(),
                    Apply::Recover,
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().sessions,
                    [created],
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn preserves_position_through_rewind_and_reorder() {
    for (case_name, run) in [
        (
            "compound_updates_preserve_order",
            (|case_name| {
                let first = session();
                let second = session();
                let backup = session();
                let mut state = snapshot(0);
                state.sessions = vec![second.clone(), first.clone()];
                let mut projection = Projection::new(state.node, 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(state)))
                    .unwrap();
                let changed = Update::Event(EventEnvelope {
                    node: NodeId([1; 32]),
                    cursor: 1,
                    event: Event::SessionRewound {
                        session: Box::new(first.clone()),
                        backup: Some(Box::new(backup.clone())),
                    },
                });
                projection.apply(1, changed.clone()).unwrap();
                assert_eq!(
                    projection.apply(1, changed).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().sessions,
                    [backup.clone(), second.clone(), first.clone()],
                    "{case_name}"
                );
                let turn = QueuedTurn {
                    id: TurnId::new(),
                    kind: conversation::RunKind::Task,
                    session: second.id,
                    request: RequestId::new(),
                    revision: second.revision,
                    config: second.config.clone(),
                    roles: second.roles.clone(),
                    plugins: vec![],
                };
                projection
                    .apply(
                        1,
                        Update::Event(EventEnvelope {
                            node: NodeId([1; 32]),
                            cursor: 2,
                            event: Event::PlanAccepted(Box::new(question::AcceptedPlan {
                                question: question::Question {
                                    id: QuestionId::new(),
                                    session: second.id,
                                    turn: TurnId::new(),
                                    entry: "plan".into(),
                                    index: 0,
                                    state: question::State::Answered(question::Answer::Plan {
                                        turn: turn.id,
                                    }),
                                },
                                session: second.clone(),
                                turn,
                            })),
                        }),
                    )
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().sessions,
                    [backup, second, first],
                    "{case_name}"
                );
                let recovered = projection.snapshot().unwrap().clone();
                projection.reconnect(2).unwrap();
                projection
                    .apply(2, Update::Snapshot(Box::new(recovered.clone())))
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().sessions,
                    recovered.sessions,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "rewind_updates_without_creating_session",
            (|case_name| {
                let first = session();
                let second = session();
                let mut state = snapshot(0);
                state.sessions = vec![second.clone(), first.clone()];
                let mut projection = Projection::new(state.node, 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(state)))
                    .unwrap();
                let mut updated = first;
                updated.activity.title = "Retained history".into();
                let changed = Update::Event(EventEnvelope {
                    node: NodeId([1; 32]),
                    cursor: 1,
                    event: Event::SessionRewound {
                        session: Box::new(updated.clone()),
                        backup: None,
                    },
                });
                projection.apply(1, changed.clone()).unwrap();
                assert_eq!(
                    projection.apply(1, changed).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().sessions,
                    [second, updated],
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "updates_keep_position_and_reorders_recover",
            (|case_name| {
                let first = session();
                let mut second = session();
                let newest = session();
                let mut state = snapshot(0);
                state.sessions = vec![second.clone(), first.clone()];
                let mut projection = Projection::new(state.node, 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(state)))
                    .unwrap();
                let mut cursor = 0;
                let mut apply = |projection: &mut Projection, event| {
                    cursor += 1;
                    projection
                        .apply(
                            1,
                            Update::Event(EventEnvelope {
                                node: NodeId([1; 32]),
                                cursor,
                                event,
                            }),
                        )
                        .unwrap()
                };
                let mut changed = first.clone();
                changed.activity.title = "Renamed".into();
                apply(
                    &mut projection,
                    Event::SessionChanged(Box::new(changed.clone())),
                );
                assert_eq!(
                    projection.snapshot().unwrap().sessions,
                    [second.clone(), changed.clone()],
                    "{case_name}"
                );
                second.activity.title = "Active".into();
                apply(
                    &mut projection,
                    Event::ConversationChanged {
                        session: second.id,
                        activity: Box::new(second.activity.clone()),
                    },
                );
                apply(
                    &mut projection,
                    Event::SessionsReordered(vec![first.id, second.id]),
                );
                assert_eq!(
                    projection.snapshot().unwrap().sessions,
                    [changed.clone(), second.clone()],
                    "{case_name}"
                );
                apply(
                    &mut projection,
                    Event::SessionChanged(Box::new(newest.clone())),
                );
                assert_eq!(
                    projection.snapshot().unwrap().sessions,
                    [newest, changed, second],
                    "{case_name}"
                );
                assert_eq!(
                    apply(
                        &mut projection,
                        Event::SessionsReordered(vec![first.id, first.id])
                    ),
                    Apply::Recover,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
