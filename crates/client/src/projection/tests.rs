use super::*;
use sailry_protocol::{Defaults, EventEnvelope};
mod notifications;
mod plugins;
mod quotas;
mod roles;
mod sessions;
mod teams;
mod worktrees;

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
        power_settings_revision: 0,
        sandbox_settings_revision: 0,
        team_settings_revision: 0,
        compaction_settings_revision: 0,
        search_settings_revision: 0,
        media_settings: Default::default(),
        providers: vec![],
        provider_quotas: vec![],
        model_catalog: Default::default(),
        roles: vec![],
        teams: vec![],
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
fn orders_and_deduplicates_shared_cursors() {
    for (case_name, run) in [
        (
            "duplicates_do_not_change_state",
            (|case_name| {
                let mut projection = Projection::new(NodeId([1; 32]), 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(snapshot(0))))
                    .unwrap();
                assert_eq!(
                    projection.apply(1, event(1)).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(
                    projection.apply(1, event(1)).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().defaults.revision,
                    1,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "snapshot_covers_buffered_events",
            (|case_name| {
                let mut state = snapshot(3);
                state.defaults.revision = 3;
                let mut projection = Projection::new(state.node, 1);
                assert_eq!(
                    projection
                        .apply(1, Update::Snapshot(Box::new(state.clone())))
                        .unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                for cursor in 1..=3 {
                    assert_eq!(
                        projection.apply(1, event(cursor)).unwrap(),
                        Apply::Ignored,
                        "{case_name}"
                    );
                    assert_eq!(projection.snapshot(), Some(&state), "{case_name}");
                }
                assert_eq!(
                    projection.apply(1, event(4)).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().defaults.revision,
                    4,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "dispatch_invalidation_advances_shared_cursor",
            (|case_name| {
                let mut projection = Projection::new(NodeId([1; 32]), 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(snapshot(0))))
                    .unwrap();
                let update = Update::Event(EventEnvelope {
                    node: NodeId([1; 32]),
                    cursor: 1,
                    event: Event::DispatchChanged {
                        packages: vec!["example".into()],
                    },
                });
                assert_eq!(
                    projection.apply(1, update.clone()).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap().cursor, 1, "{case_name}");
                assert_eq!(
                    projection.apply(1, update).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(
                    projection.apply(1, event(2)).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "resource_changes_advance_shared_cursor",
            (|case_name| {
                let node = NodeId([1; 32]);
                let mut projection = Projection::new(node, 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(snapshot(0))))
                    .unwrap();
                for (index, event) in [
                    Event::LocalModelsChanged { models: vec![] },
                    Event::LocalModelSettingsChanged { revision: 3 },
                    Event::TelemetrySettingsChanged { revision: 3 },
                    Event::KnowledgeCollectionsChanged,
                ]
                .into_iter()
                .enumerate()
                {
                    let cursor = index as u64 + 1;
                    let update = Update::Event(EventEnvelope {
                        node,
                        cursor,
                        event,
                    });
                    assert_eq!(
                        projection.apply(1, update.clone()).unwrap(),
                        Apply::Applied,
                        "{case_name}"
                    );
                    assert_eq!(
                        projection.snapshot(),
                        Some(&snapshot(cursor)),
                        "{case_name}"
                    );
                    assert_eq!(
                        projection.apply(1, update).unwrap(),
                        Apply::Ignored,
                        "{case_name}"
                    );
                }
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn rejects_invalid_subscription_updates() {
    for (case_name, run) in [
        (
            "rejects_status_on_node_subscription",
            (|case_name| {
                let node = NodeId([1; 32]);
                let mut projection = Projection::new(node, 1);
                let initial = snapshot(0);
                projection
                    .apply(1, Update::Snapshot(Box::new(initial.clone())))
                    .unwrap();
                let update = Update::Status(sailry_protocol::plugin::ui::status::Snapshot {
                    node,
                    scope: sailry_protocol::plugin::ui::status::Scope {
                        worktree: None,
                        session: Some(sailry_protocol::SessionId::new()),
                    },
                    entries: vec![],
                    failures: vec![],
                });
                assert_eq!(
                    projection.apply(1, update).unwrap_err().code,
                    sailry_protocol::ErrorCode::InvalidRequest,
                    "{case_name}"
                );
                assert_eq!(projection.snapshot(), Some(&initial), "{case_name}");
            }) as fn(&str),
        ),
        (
            "rejects_unknown_event_updates",
            (|case_name| {
                let node = NodeId([1; 32]);
                let mut projection = Projection::new(node, 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(snapshot(0))))
                    .unwrap();
                let update = serde_json::from_value::<Update>(serde_json::json!({
                    "kind": "event",
                    "data": {
                        "node": node,
                        "cursor": 1,
                        "event": { "kind": "future_event", "data": { "private": "value" } }
                    }
                }));
                assert!(update.is_err(), "{case_name}");
                assert_eq!(projection.snapshot(), Some(&snapshot(0)), "{case_name}");
                assert_eq!(
                    projection.apply(1, event(1)).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "rejects_foreign_or_stale_snapshots",
            (|case_name| {
                let mut projection = Projection::new(NodeId([1; 32]), 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(snapshot(2))))
                    .unwrap();
                assert_eq!(
                    projection
                        .apply(1, Update::Snapshot(Box::new(snapshot(1))))
                        .unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                let mut other = snapshot(3);
                other.node = NodeId([2; 32]);
                assert_eq!(
                    projection
                        .apply(1, Update::Snapshot(Box::new(other)))
                        .unwrap_err()
                        .code,
                    ErrorCode::WrongTarget,
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap().cursor, 2, "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn recovers_gaps_and_replaced_generations() {
    for (case_name, run) in [
        (
            "gaps_require_snapshot",
            (|case_name| {
                let mut projection = Projection::new(NodeId([1; 32]), 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(snapshot(0))))
                    .unwrap();
                assert_eq!(
                    projection.apply(1, event(2)).unwrap(),
                    Apply::Recover,
                    "{case_name}"
                );
                assert_eq!(
                    projection.apply(1, event(1)).unwrap(),
                    Apply::Recover,
                    "{case_name}"
                );
                projection
                    .apply(1, Update::Snapshot(Box::new(snapshot(2))))
                    .unwrap();
                assert_eq!(
                    projection.apply(1, event(3)).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "rejects_previous_generation",
            (|case_name| {
                let mut projection = Projection::new(NodeId([1; 32]), 1);
                projection
                    .apply(1, Update::Snapshot(Box::new(snapshot(0))))
                    .unwrap();
                projection.reconnect(2).unwrap();
                assert_eq!(
                    projection.apply(1, event(1)).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(
                    projection
                        .apply(1, Update::Snapshot(Box::new(snapshot(99))))
                        .unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                projection
                    .apply(2, Update::Snapshot(Box::new(snapshot(1))))
                    .unwrap();
                assert_eq!(
                    projection.apply(2, event(2)).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert!(projection.reconnect(2).is_err(), "{case_name}");
            }) as fn(&str),
        ),
        (
            "reconnects_to_restored_node_cursor",
            (|case_name| {
                let mut projection = Projection::new(NodeId([1; 32]), 1);
                let mut current = snapshot(100);
                current.defaults.revision = 9;
                projection
                    .apply(1, Update::Snapshot(Box::new(current.clone())))
                    .unwrap();
                projection.reconnect(2).unwrap();
                assert_eq!(
                    projection
                        .apply(1, Update::Snapshot(Box::new(snapshot(200))))
                        .unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                let mut restored = snapshot(20);
                restored.defaults.revision = 3;
                assert_eq!(
                    projection
                        .apply(2, Update::Snapshot(Box::new(restored.clone())))
                        .unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(projection.snapshot(), Some(&restored), "{case_name}");
                assert_eq!(
                    projection.apply(2, event(21)).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().defaults.revision,
                    21,
                    "{case_name}"
                );
                assert_eq!(
                    projection
                        .apply(2, Update::Snapshot(Box::new(snapshot(19))))
                        .unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap().cursor, 21, "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
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
        access: Some(sailry_protocol::process::Access::Workspace),
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
    projection
        .apply(1, Update::Snapshot(Box::new(state)))
        .unwrap();
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

#[test]
fn invalidates_credentials_on_revision_and_recovery() {
    let node = NodeId([1; 32]);
    let mut projection = Projection::new(node, 1);
    projection
        .apply(1, Update::Snapshot(Box::new(snapshot(0))))
        .unwrap();
    let initial = projection.credential_cursor();
    let credential = sailry_protocol::CredentialRef {
        node,
        id: sailry_protocol::CredentialId::new(),
    };
    for revision in [7, 8] {
        let cursor = revision - 6;
        let event = Update::Event(EventEnvelope {
            node,
            cursor,
            event: Event::CredentialChanged {
                credential: credential.clone(),
                revision,
            },
        });
        let before = projection.credential_cursor();
        assert_eq!(projection.apply(1, event.clone()).unwrap(), Apply::Applied);
        assert!(projection.credential_cursor() > before);
        assert_eq!(projection.apply(1, event).unwrap(), Apply::Ignored);
        assert_eq!(projection.credential_cursor(), before + 1);
        assert!(projection.snapshot().unwrap().providers.is_empty());
    }
    assert!(projection.credential_cursor() > initial);
    let changed = projection.credential_cursor();
    assert_eq!(
        projection.apply(1, Update::ResetRequired).unwrap(),
        Apply::Recover
    );
    assert!(projection.credential_cursor() > changed);
    let reset = projection.credential_cursor();
    projection
        .apply(1, Update::Snapshot(Box::new(snapshot(2))))
        .unwrap();
    assert!(projection.credential_cursor() > reset);
    let recovered = projection.credential_cursor();
    projection.reconnect(2).unwrap();
    assert!(projection.credential_cursor() > recovered);
    let reconnect = projection.credential_cursor();
    assert_eq!(projection.apply(1, event(3)).unwrap(), Apply::Ignored);
    assert_eq!(projection.credential_cursor(), reconnect);
}

#[test]
fn power_policy_revision_follows_authority() {
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    projection
        .apply(1, Update::Snapshot(Box::new(snapshot(0))))
        .unwrap();
    projection
        .apply(
            1,
            Update::Event(EventEnvelope {
                node: NodeId([1; 32]),
                cursor: 1,
                event: Event::PowerSettingsChanged { revision: 3 },
            }),
        )
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().power_settings_revision, 3);
    assert_eq!(
        projection
            .apply(
                1,
                Update::Event(EventEnvelope {
                    node: NodeId([1; 32]),
                    cursor: 1,
                    event: Event::PowerSettingsChanged { revision: 1 }
                })
            )
            .unwrap(),
        Apply::Ignored
    );
    assert_eq!(projection.snapshot().unwrap().power_settings_revision, 3);
}
