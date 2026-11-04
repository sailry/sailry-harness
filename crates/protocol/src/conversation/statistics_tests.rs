use super::*;

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "defaults_missing_delegated_usage",
            (|scenario: &'static str| {
                let mut wire = serde_json::to_value(Statistics::default()).unwrap();
                wire.as_object_mut().unwrap().remove("delegated");
                assert_eq!(
                    serde_json::from_value::<Statistics>(wire).unwrap(),
                    Statistics::default(),
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "preserves_change_shape",
            (|scenario: &'static str| {
                let statistics = Statistics {
                    responses: 3,
                    observations: observation::Summary {
                        latency: observation::Latency {
                            first_token_us: Some(0),
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
                    serde_json::json!({ "kind": "statistics", "data": data }),
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<Change>(wire).unwrap(),
                    change,
                    "{scenario}"
                );
                let error = Fault::new(crate::ErrorCode::Internal, "statistics read failed");
                let failed = Change::StatisticsFailed(error.clone());
                let wire = serde_json::to_value(&failed).unwrap();
                assert_eq!(
                    wire,
                    serde_json::json!({"kind":"statistics_failed","data":error}),
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<Change>(wire).unwrap(),
                    failed,
                    "{scenario}"
                );
            }),
        ),
        (
            "preserves_update_shape",
            (|scenario: &'static str| {
                let snapshot = Snapshot {
                    node: NodeId([61; 32]),
                    sequence: 3,
                    page: std::sync::Arc::new(Page {
                        session: SessionId::new(),
                        revision: 1,
                        entries: Vec::new().into(),
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
                    statistics: Some(Statistics {
                        responses: 3,
                        ..Default::default()
                    }),
                    artifacts: Default::default(),
                };
                let mut pending = snapshot.clone();
                pending.statistics = None;
                let wire = serde_json::to_value(&pending).unwrap();
                assert!(wire.get("statistics").unwrap().is_null(), "{scenario}");
                assert_eq!(
                    serde_json::from_value::<Snapshot>(wire).unwrap(),
                    pending,
                    "{scenario}"
                );
                let data = serde_json::to_value(&snapshot).unwrap();
                let update = crate::Update::ConversationSnapshot(Box::new(snapshot));
                let wire = serde_json::to_value(&update).unwrap();
                assert_eq!(
                    wire,
                    serde_json::json!({ "kind": "conversation_snapshot", "data": data }),
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<crate::Update>(wire).unwrap(),
                    update,
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
