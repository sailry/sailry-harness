use super::*;
use serde_json::json;

#[test]
fn latest_attempt_replaces_details() {
    for (detail, expected) in [
        (json!("Latest failure"), Some("Latest failure")),
        (
            json!(r#"{"error":{"message":"Unavailable"}}"#),
            Some(r#"{"error":{"message":"Unavailable"}}"#),
        ),
        (serde_json::Value::Null, None),
        (json!("  "), None),
    ] {
        let turn = TurnId::new();
        let parts = vec![
            Part::Resource(
                json!({"type":"model_retry","attempt":1,"limit":5,"error":"First failure"}),
            ),
            Part::Resource(json!({"type":"model_retry","attempt":2,"limit":5,"error":detail})),
            Part::Text("Response".into()),
            Part::Resource(json!({"type":"model_retry","attempt":1,"limit":5})),
        ];
        let history = View {
            snapshot: Some(Arc::new(Snapshot {
                node: NodeId([1; 32]),
                sequence: 1,
                missing: vec![],
                drafts: vec![],
                statistics: Default::default(),
                artifacts: Default::default(),
                page: Arc::new(Page {
                    session: SessionId::new(),
                    revision: 1,
                    next_before: None,
                    queue: Default::default(),
                    approvals: vec![],
                    questions: vec![],
                    children: vec![],
                    background: vec![],
                    runs: vec![],
                    entries: vec![Entry {
                        id: "retry".into(),
                        sequence: 1,
                        turn,
                        author: "system".into(),
                        branch: String::new(),
                        timestamp_ms: 1,
                        parts: parts.clone(),
                        usage: None,
                        citations: vec![],
                        search_suggestions: None,
                    }],
                }),
            })),
            ..Default::default()
        };
        let blocks = collect(turn, &history);
        assert_eq!(blocks.len(), 3);
        assert!(
            matches!(&blocks[0], Block::Retry(key, 2, 5, error) if key == "retry-0" && *error == expected)
        );
        assert!(matches!(&blocks[2], Block::Retry(key, 1, 5, None) if key == "retry-3"));
        assert_eq!(
            history.snapshot.as_ref().unwrap().page.entries[0].parts,
            parts
        );
    }
}
