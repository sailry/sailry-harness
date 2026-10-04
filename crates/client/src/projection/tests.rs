use super::*;
use sailry_protocol::{Defaults, EventEnvelope};
mod notifications;
mod plugins;
mod roles;
mod sessions;

fn snapshot(cursor: u64) -> Snapshot {
    Snapshot {
        node: NodeId([1; 32]),
        cursor,
        defaults: Defaults {
            revision: 0,
            config: None,
        },
        projects: vec![],
        worktrees: vec![],
        sessions: vec![],
        terminals: vec![],
        terminal_settings_revision: 0,
        media_settings: Default::default(),
        providers: vec![],
        model_catalog: Default::default(),
        roles: vec![],
        ssh: vec![],
        databases: vec![],
        plugins: vec![],
        notifications: vec![],
        turns: vec![],
    }
}

fn event(cursor: u64) -> Update {
    Update::Event(EventEnvelope {
        node: NodeId([1; 32]),
        cursor,
        event: Event::DefaultsChanged(Defaults {
            revision: cursor,
            config: None,
        }),
    })
}

#[test]
fn duplicates_do_not_change_state() {
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    projection.apply(1, Update::Snapshot(snapshot(0))).unwrap();
    assert_eq!(projection.apply(1, event(1)).unwrap(), Apply::Applied);
    assert_eq!(projection.apply(1, event(1)).unwrap(), Apply::Ignored);
    assert_eq!(projection.snapshot().unwrap().defaults.revision, 1);
}

#[test]
fn dispatch_invalidation_advances_shared_cursor() {
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    projection.apply(1, Update::Snapshot(snapshot(0))).unwrap();
    let update = Update::Event(EventEnvelope {
        node: NodeId([1; 32]),
        cursor: 1,
        event: Event::DispatchChanged {
            packages: vec!["example".into()],
        },
    });
    assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
    assert_eq!(projection.snapshot().unwrap().cursor, 1);
    assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
    assert_eq!(projection.apply(1, event(2)).unwrap(), Apply::Applied);
}

#[test]
fn unknown_events_only_advance_cursor() {
    let node = NodeId([1; 32]);
    let mut projection = Projection::new(node, 1);
    projection.apply(1, Update::Snapshot(snapshot(0))).unwrap();
    let update: Update = serde_json::from_value(serde_json::json!({
        "kind": "event",
        "data": {
            "node": node,
            "cursor": 1,
            "event": { "kind": "future_event", "data": { "private": "value" } }
        }
    }))
    .unwrap();
    assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
    assert_eq!(projection.snapshot(), Some(&snapshot(1)));
    assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
    assert_eq!(projection.apply(1, event(2)).unwrap(), Apply::Applied);
    assert_eq!(projection.snapshot().unwrap().defaults.revision, 2);
    assert_eq!(projection.apply(1, event(4)).unwrap(), Apply::Recover);
}

#[test]
fn gaps_require_snapshot() {
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    projection.apply(1, Update::Snapshot(snapshot(0))).unwrap();
    assert_eq!(projection.apply(1, event(2)).unwrap(), Apply::Recover);
    assert_eq!(projection.apply(1, event(1)).unwrap(), Apply::Recover);
    projection.apply(1, Update::Snapshot(snapshot(2))).unwrap();
    assert_eq!(projection.apply(1, event(3)).unwrap(), Apply::Applied);
}

#[test]
fn rejects_previous_generation() {
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    projection.apply(1, Update::Snapshot(snapshot(0))).unwrap();
    projection.reconnect(2).unwrap();
    assert_eq!(projection.apply(1, event(1)).unwrap(), Apply::Ignored);
    assert_eq!(
        projection.apply(1, Update::Snapshot(snapshot(99))).unwrap(),
        Apply::Ignored
    );
    projection.apply(2, Update::Snapshot(snapshot(1))).unwrap();
    assert_eq!(projection.apply(2, event(2)).unwrap(), Apply::Applied);
    assert!(projection.reconnect(2).is_err());
}

#[test]
fn rejects_foreign_or_stale_snapshots() {
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    projection.apply(1, Update::Snapshot(snapshot(2))).unwrap();
    assert_eq!(
        projection.apply(1, Update::Snapshot(snapshot(1))).unwrap(),
        Apply::Ignored
    );
    let mut other = snapshot(3);
    other.node = NodeId([2; 32]);
    assert_eq!(
        projection
            .apply(1, Update::Snapshot(other))
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
    assert_eq!(projection.snapshot().unwrap().cursor, 2);
}

#[test]
fn terminal_updates_preserve_order() {
    use sailry_protocol::{
        TerminalId,
        terminal::{Info, Status},
    };
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    let first = Info {
        id: TerminalId::new(),
        worktree: None,
        ssh: None,
        tool: None,
        status: Status::Running,
        activity: None,
        title: None,
        directory: None,
        owner: None,
        revision: 1,
    };
    let second = Info {
        id: TerminalId::new(),
        ..first.clone()
    };
    let mut state = snapshot(0);
    state.terminals = vec![first.clone(), second.clone()];
    projection.apply(1, Update::Snapshot(state)).unwrap();
    let mut changed = first.clone();
    changed.title = Some("Server".into());
    projection
        .apply(
            1,
            Update::Event(EventEnvelope {
                node: NodeId([1; 32]),
                cursor: 1,
                event: Event::TerminalChanged(changed.clone()),
            }),
        )
        .unwrap();
    assert_eq!(
        projection.snapshot().unwrap().terminals,
        vec![changed, second]
    );
}
