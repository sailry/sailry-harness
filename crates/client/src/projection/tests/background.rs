use super::*;

#[test]
fn projects_execution_node_settings_revision() {
    let mut projection = Projection::new(NodeId([1; 32]), 1);
    projection.apply(1, Update::Snapshot(snapshot(0))).unwrap();
    projection
        .apply(
            1,
            Update::Event(EventEnvelope {
                node: NodeId([1; 32]),
                cursor: 1,
                event: Event::BackgroundSettingsChanged { revision: 7 },
            }),
        )
        .unwrap();
    assert_eq!(
        projection.snapshot().unwrap().background_settings_revision,
        7
    );
}
