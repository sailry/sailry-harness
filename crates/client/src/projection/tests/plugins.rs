use super::*;
use sailry_protocol::plugin::Summary;

#[test]
fn reduces_inventory_in_node_order() {
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    projection.apply(1, Update::Snapshot(snapshot(0))).unwrap();
    let plugin = Summary {
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
