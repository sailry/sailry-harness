use super::*;
use crate::preferences::pins::{State, Target};
use core::prelude::v1::test;
use sailry_protocol::{ProjectId, ProviderId, SessionId, TerminalId, WorktreeId};

fn session(created: u64, timestamp: i64) -> Session {
    let id = SessionId::new();
    let worktree = WorktreeId::new();
    serde_json::from_value(serde_json::json!({
        "id": id, "creation_order": created, "archived": false,
        "activity": {
            "title": "Fixture", "attention": {"revision": 0, "unread": false},
            "queued": 0,
            "run": {
                "worktree": worktree, "session": id, "turn": sailry_protocol::TurnId::new(),
                "kind": "task", "sequence": 1, "revision": 1, "status": "completed",
                "started_ms": timestamp, "finished_ms": timestamp,
            }
        },
        "project": null, "worktree": worktree, "revision": 1,
        "config": {
            "provider": ProviderId::new(), "model": "fixture",
            "mode": "code", "permission": "ask"
        },
        "roles": {"profiles": [], "providers": []}
    }))
    .unwrap()
}

#[test]
fn combines_pins_and_selected_host_activity() {
    let node = NodeId([1; 32]);
    let other = NodeId([2; 32]);
    let first = session(1, 10);
    let second = session(2, 20);
    let running = session(3, 30);
    let foreign = session(4, 40);
    let sessions = [first.clone(), second.clone(), running.clone()];
    let rows = compose(
        node,
        &sessions,
        [],
        vec![
            (node, second.clone()),
            (node, running.clone()),
            (other, foreign),
        ],
        |id| {
            if id == Target::Session(first.id) {
                State {
                    pinned: true,
                    order: Some(2),
                }
            } else if id == Target::Session(second.id) {
                State {
                    pinned: true,
                    order: Some(1),
                }
            } else {
                State::default()
            }
        },
    );
    assert_eq!(
        rows.iter().map(Entry::target).collect::<Vec<_>>(),
        [second.id, first.id, running.id].map(Target::Session)
    );
}

#[test]
fn orders_legacy_pins_before_new_pins() {
    let older = session(3, 10);
    let newer = session(1, 20);
    let appended = session(2, 30);
    let rows = compose(
        NodeId([1; 32]),
        &[older.clone(), appended.clone(), newer.clone()],
        [],
        vec![],
        |id| State {
            pinned: true,
            order: (id == Target::Session(appended.id)).then_some(0),
        },
    );
    assert_eq!(
        rows.iter().map(Entry::target).collect::<Vec<_>>(),
        [newer.id, older.id, appended.id].map(Target::Session)
    );
}

#[test]
fn hides_empty_and_archived_entries() {
    let node = NodeId([1; 32]);
    assert!(compose(node, &[], [], vec![], |_| State::default()).is_empty());
    let mut archived = session(1, 1);
    archived.archived = true;
    assert!(
        compose(node, &[archived], [], vec![], |_| {
            State {
                pinned: true,
                order: None,
            }
        })
        .is_empty()
    );
}

#[test]
fn retains_pins_in_resource_lists() {
    let node = NodeId([1; 32]);
    let mut session = session(1, 1);
    for project in [None, Some(ProjectId::new())] {
        session.project = project;
        let rows = compose(node, std::slice::from_ref(&session), [], vec![], |_| {
            State {
                pinned: true,
                order: Some(0),
            }
        });
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target(), Target::Session(session.id));
        assert!(super::super::tree::listed(&session));
    }
}

#[test]
fn interleaves_terminal_and_session_pins() {
    let node = NodeId([1; 32]);
    let first = session(1, 10);
    let last = session(2, 20);
    let active = session(3, 30);
    let terminal = Info {
        id: TerminalId::new(),
        worktree: Some(first.worktree),
        ssh: None,
        tool: None,
        access: None,
        status: Status::Running,
        activity: None,
        title: Some("Fixture".into()),
        directory: None,
        owner: Some(node),
        revision: 1,
    };
    let mut closed = terminal.clone();
    closed.id = TerminalId::new();
    closed.status = Status::Closed;
    let mut unpinned = terminal.clone();
    unpinned.id = TerminalId::new();
    let order = [
        Target::Session(first.id),
        Target::Terminal(terminal.id),
        Target::Session(last.id),
        Target::Terminal(closed.id),
    ];
    let rows = compose(
        node,
        &[last, first.clone()],
        [(None, &terminal), (None, &closed), (None, &unpinned)],
        vec![(node, active.clone()), (node, first)],
        |target| {
            let order = order.iter().position(|candidate| *candidate == target);
            State {
                pinned: order.is_some(),
                order: order.map(|order| order as u64),
            }
        },
    );
    assert_eq!(
        rows.iter().map(Entry::target).collect::<Vec<_>>(),
        [order[0], order[1], order[2], active.id.into()]
    );
}
