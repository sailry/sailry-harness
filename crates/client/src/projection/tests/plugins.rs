use super::*;
use sailry_protocol::plugin::Summary;

#[test]
fn catalog_reads_ignore_conversation_traffic() {
    use sailry_protocol::plugin::Action;
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    projection.apply(1, Update::Snapshot(snapshot(0))).unwrap();
    let roles = [Action::ReadRoles];
    let models = [Action::ReadModels];
    let terminals = [Action::ReadTerminals];
    let initial = projection.read_cursors();
    projection.apply(1, event(1)).unwrap();
    assert_eq!(projection.read_cursors(), initial);
    for (cursor, event, actions) in [
        (
            2,
            Event::RoleRemoved {
                id: sailry_protocol::RoleId::new(),
            },
            &roles[..],
        ),
        (
            3,
            Event::ProviderRemoved {
                id: sailry_protocol::ProviderId::new(),
            },
            &models[..],
        ),
        (
            4,
            Event::TerminalSettingsChanged { revision: 1 },
            &terminals[..],
        ),
    ] {
        let before = projection.read_cursors().for_actions(actions);
        projection
            .apply(
                1,
                Update::Event(EventEnvelope {
                    node: NodeId([1; 32]),
                    cursor,
                    event,
                }),
            )
            .unwrap();
        assert!(projection.read_cursors().for_actions(actions) > before);
    }
    let before = projection.read_cursors();
    projection.apply(1, event(5)).unwrap();
    assert_eq!(projection.read_cursors(), before);
    projection.apply(1, Update::ResetRequired).unwrap();
    projection.apply(1, Update::Snapshot(snapshot(5))).unwrap();
    assert!(projection.read_cursors().for_actions(&roles) > before.for_actions(&roles));
    assert_eq!(projection.read_cursors().for_actions(&[Action::ReadGit]), 0);
}

#[test]
fn reduces_inventory_in_node_order() {
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    projection.apply(1, Update::Snapshot(snapshot(0))).unwrap();
    let plugin = Summary {
        compatibility: Default::default(),
        name: "analysis".into(),
        revision: 1,
        digest: "a".repeat(64),
        settings_revision: 0,
        enabled: true,
        version: None,
        description: None,
    };
    let changed = Update::Event(EventEnvelope {
        node: NodeId([1; 32]),
        cursor: 1,
        event: Event::PluginChanged(plugin.clone()),
    });
    assert_eq!(
        projection.apply(1, changed.clone()).unwrap(),
        Apply::Applied
    );
    assert_eq!(projection.apply(1, changed).unwrap(), Apply::Ignored);
    assert_eq!(
        projection.snapshot().unwrap().plugins.as_slice(),
        std::slice::from_ref(&plugin)
    );
    let configured = Summary {
        revision: 2,
        settings_revision: 1,
        ..plugin
    };
    projection
        .apply(
            1,
            Update::Event(EventEnvelope {
                node: NodeId([1; 32]),
                cursor: 2,
                event: Event::PluginChanged(configured.clone()),
            }),
        )
        .unwrap();
    assert_eq!(projection.snapshot().unwrap().plugins, [configured]);
    let removed = Update::Event(EventEnvelope {
        node: NodeId([1; 32]),
        cursor: 3,
        event: Event::PluginRemoved {
            name: "analysis".into(),
        },
    });
    assert_eq!(projection.apply(1, removed).unwrap(), Apply::Applied);
    assert!(projection.snapshot().unwrap().plugins.is_empty());
}

#[test]
fn package_cursors_follow_accepted_changes() {
    let node = NodeId([1; 32]);
    let mut projection = Projection::new(node, 1);
    let mut state = snapshot(10);
    state.plugins = ["example", "other"]
        .into_iter()
        .map(|name| Summary {
            compatibility: Default::default(),
            name: name.into(),
            revision: 1,
            digest: "a".repeat(64),
            settings_revision: 0,
            enabled: true,
            version: None,
            description: None,
        })
        .collect();
    projection
        .apply(1, Update::Snapshot(state.clone()))
        .unwrap();
    assert_eq!(projection.package_cursors()["example"], 1);
    projection.apply(1, event(11)).unwrap();
    assert_eq!(projection.package_cursors()["example"], 1);
    let changed = Update::Event(EventEnvelope {
        node,
        cursor: 12,
        event: Event::PluginValuesChanged {
            name: "example".into(),
        },
    });
    assert_eq!(
        projection.apply(1, changed.clone()).unwrap(),
        Apply::Applied
    );
    assert_eq!(projection.package_cursors()["example"], 2);
    assert_eq!(projection.package_cursors()["other"], 1);
    assert_eq!(projection.apply(1, changed).unwrap(), Apply::Ignored);
    projection.reconnect(2).unwrap();
    assert_eq!(projection.apply(1, event(13)).unwrap(), Apply::Ignored);
    state.cursor = 15;
    projection
        .apply(2, Update::Snapshot(state.clone()))
        .unwrap();
    assert_eq!(projection.package_cursors()["example"], 3);
    assert_eq!(projection.package_cursors()["other"], 3);
    // Recovery must invalidate even when the Node cursor and connection stay the same.
    assert_eq!(
        projection.apply(2, Update::ResetRequired).unwrap(),
        Apply::Recover
    );
    projection.apply(2, Update::Snapshot(state)).unwrap();
    assert_eq!(projection.package_cursors()["example"], 4);
    assert_eq!(projection.package_cursors()["other"], 4);
    assert_eq!(projection.snapshot().unwrap().cursor, 15);
    projection
        .apply(
            2,
            Update::Event(EventEnvelope {
                node,
                cursor: 16,
                event: Event::PluginRemoved {
                    name: "example".into(),
                },
            }),
        )
        .unwrap();
    assert!(!projection.package_cursors().contains_key("example"));
}
