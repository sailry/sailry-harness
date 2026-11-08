use super::*;
use crate::preferences::sessions::State;
use core::prelude::v1::test;
use sailry_protocol::{ProjectId, ProviderId, WorktreeId};

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
        vec![
            (node, second.clone()),
            (node, running.clone()),
            (other, foreign),
        ],
        |id| {
            if id == first.id {
                State {
                    pinned: true,
                    order: Some(2),
                }
            } else if id == second.id {
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
        rows.iter().map(|session| session.id).collect::<Vec<_>>(),
        [second.id, first.id, running.id]
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
        vec![],
        |id| State {
            pinned: true,
            order: (id == appended.id).then_some(0),
        },
    );
    assert_eq!(
        rows.iter().map(|session| session.id).collect::<Vec<_>>(),
        [newer.id, older.id, appended.id]
    );
}

#[test]
fn hides_empty_and_archived_entries() {
    let node = NodeId([1; 32]);
    assert!(compose(node, &[], vec![], |_| State::default()).is_empty());
    let mut archived = session(1, 1);
    archived.archived = true;
    assert!(
        compose(node, &[archived], vec![], |_| State {
            pinned: true,
            order: None
        })
        .is_empty()
    );
}

#[test]
fn excludes_pins_from_resource_lists() {
    let mut session = session(1, 1);
    for project in [None, Some(ProjectId::new())] {
        session.project = project;
        assert!(super::super::tree::listed(&session, false));
        assert!(!super::super::tree::listed(&session, true));
    }
}
