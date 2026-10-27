use super::*;

#[test]
fn preserves_statistics_identity_across_drafts() {
    let node = NodeId([7; 32]);
    let session = SessionId::new();
    let mut view = View::default();
    let mut snapshot = Snapshot {
        node,
        sequence: 0,
        page: Arc::new(conversation::Page {
            session,
            revision: 1,
            entries: Vec::new(),
            runs: Vec::new(),
            queue: Default::default(),
            approvals: Vec::new(),
            questions: Vec::new(),
            children: Vec::new(),
            background: Vec::new(),
            next_before: None,
        }),
        missing: Vec::new(),
        drafts: Vec::new(),
        statistics: conversation::Statistics {
            responses: 1,
            ..Default::default()
        },
        artifacts: Default::default(),
    };
    view.replace(Arc::new(snapshot.clone()), 0, 1, None);
    let statistics = view.statistics.clone();
    snapshot.drafts.push(conversation::Draft {
        id: "draft".into(),
        turn: TurnId::new(),
        author: "assistant".into(),
        branch: String::new(),
        commentary: Default::default(),
        parts: vec![conversation::Part::Text("More text".into())],
    });
    snapshot.sequence = 1;
    view.replace(Arc::new(snapshot.clone()), 0, 1, None);
    assert!(Arc::ptr_eq(&statistics, &view.statistics));
    snapshot.statistics.responses = 2;
    view.replace(Arc::new(snapshot), 0, 1, None);
    assert!(!Arc::ptr_eq(&statistics, &view.statistics));
    assert_eq!(view.statistics.responses, 2);
}
