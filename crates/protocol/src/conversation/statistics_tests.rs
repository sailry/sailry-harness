use super::*;

#[test]
fn defaults_missing_delegated_usage() {
    let mut wire = serde_json::to_value(Statistics::default()).unwrap();
    wire.as_object_mut().unwrap().remove("delegated");
    assert_eq!(
        serde_json::from_value::<Statistics>(wire).unwrap(),
        Statistics::default()
    );
}

#[test]
fn preserves_change_shape() {
    let statistics = Statistics {
        responses: 3,
        observations: observation::Summary {
            latency: observation::Latency {
                first_text_us: Some(0),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let data = serde_json::to_value(&statistics).unwrap();
    let change = Change::Statistics(Box::new(statistics));
    let wire = serde_json::to_value(&change).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({ "kind": "statistics", "data": data })
    );
    assert_eq!(serde_json::from_value::<Change>(wire).unwrap(), change);
}

#[test]
fn preserves_update_shape() {
    let snapshot = Snapshot {
        node: NodeId([61; 32]),
        sequence: 3,
        page: std::sync::Arc::new(Page {
            session: SessionId::new(),
            revision: 1,
            entries: Vec::new(),
            runs: Vec::new(),
            queue: Queue::default(),
            approvals: Vec::new(),
            questions: Vec::new(),
            children: Vec::new(),
            background: Vec::new(),
            next_before: None,
        }),
        missing: Vec::new(),
        drafts: Vec::new(),
        statistics: Statistics {
            responses: 3,
            ..Default::default()
        },
        artifacts: Default::default(),
    };
    let data = serde_json::to_value(&snapshot).unwrap();
    let update = crate::Update::ConversationSnapshot(Box::new(snapshot));
    let wire = serde_json::to_value(&update).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({ "kind": "conversation_snapshot", "data": data })
    );
    assert_eq!(
        serde_json::from_value::<crate::Update>(wire).unwrap(),
        update
    );
}
