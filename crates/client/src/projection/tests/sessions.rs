use super::*;
use sailry_protocol::{conversation::question, *};

fn session() -> Session {
    Session {
        id: SessionId::new(),
        archived: false,
        activity: Default::default(),
        project: Some(ProjectId::new()),
        worktree: WorktreeId::new(),
        revision: 1,
        config: SessionConfig {
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "fixture".into(),
            effort: Effort::Default,
            mode: WorkMode::Code,
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
fn scheduled_session_recovers_workspace_and_keeps_turn() {
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
        projection.apply(1, Update::Snapshot(state)).unwrap();
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
            }
        );
        if known {
            assert_eq!(projection.snapshot().unwrap().sessions, vec![created]);
            assert_eq!(projection.snapshot().unwrap().turns, vec![turn]);
            assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
        }
    }
}

#[test]
fn new_unassigned_workspace_recovers_from_node() {
    let mut created = session();
    created.project = None;
    let mut state = snapshot(0);
    let mut projection = Projection::new(state.node, 1);
    projection
        .apply(1, Update::Snapshot(state.clone()))
        .unwrap();
    let event = Update::Event(EventEnvelope {
        node: state.node,
        cursor: 1,
        event: Event::SessionChanged(Box::new(created.clone())),
    });
    assert_eq!(projection.apply(1, event).unwrap(), Apply::Recover);
    state.cursor = 1;
    state.worktrees.push(Worktree {
        id: created.worktree,
        project: None,
        path: "/fixture/sessions/workspace".into(),
        main: false,
    });
    state.sessions.push(created.clone());
    projection.apply(1, Update::Snapshot(state)).unwrap();
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
        Apply::Recover
    );
    assert_eq!(projection.snapshot().unwrap().sessions, [created]);
}

#[test]
fn compound_updates_preserve_order() {
    let first = session();
    let second = session();
    let backup = session();
    let mut state = snapshot(0);
    state.sessions = vec![second.clone(), first.clone()];
    let mut projection = Projection::new(state.node, 1);
    projection.apply(1, Update::Snapshot(state)).unwrap();
    let changed = Update::Event(EventEnvelope {
        node: NodeId([1; 32]),
        cursor: 1,
        event: Event::SessionRewound {
            session: Box::new(first.clone()),
            backup: Box::new(backup.clone()),
        },
    });
    projection.apply(1, changed.clone()).unwrap();
    assert_eq!(projection.apply(1, changed).unwrap(), Apply::Ignored);
    assert_eq!(
        projection.snapshot().unwrap().sessions,
        [backup.clone(), second.clone(), first.clone()]
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
                        state: question::State::Answered(question::Answer::Plan { turn: turn.id }),
                    },
                    session: second.clone(),
                    turn,
                })),
            }),
        )
        .unwrap();
    assert_eq!(
        projection.snapshot().unwrap().sessions,
        [backup, second, first]
    );
    let recovered = projection.snapshot().unwrap().clone();
    projection.reconnect(2).unwrap();
    projection
        .apply(2, Update::Snapshot(recovered.clone()))
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().sessions, recovered.sessions);
}

#[test]
fn updates_keep_position_and_reorders_recover() {
    let first = session();
    let mut second = session();
    let newest = session();
    let mut state = snapshot(0);
    state.sessions = vec![second.clone(), first.clone()];
    let mut projection = Projection::new(state.node, 1);
    projection.apply(1, Update::Snapshot(state)).unwrap();
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
        [second.clone(), changed.clone()]
    );
    second.activity.title = "Active".into();
    apply(
        &mut projection,
        Event::ConversationChanged {
            session: second.id,
            activity: second.activity.clone(),
        },
    );
    apply(
        &mut projection,
        Event::SessionsReordered(vec![first.id, second.id]),
    );
    assert_eq!(
        projection.snapshot().unwrap().sessions,
        [changed.clone(), second.clone()]
    );
    apply(
        &mut projection,
        Event::SessionChanged(Box::new(newest.clone())),
    );
    assert_eq!(
        projection.snapshot().unwrap().sessions,
        [newest, changed, second]
    );
    assert_eq!(
        apply(
            &mut projection,
            Event::SessionsReordered(vec![first.id, first.id])
        ),
        Apply::Recover
    );
}
