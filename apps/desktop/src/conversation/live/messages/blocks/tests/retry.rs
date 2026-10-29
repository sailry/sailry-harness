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
        let history = history(turn, parts.clone());
        let blocks = collect(turn, &history);
        assert_eq!(blocks.len(), 3);
        assert!(
            matches!(&blocks[0], Block::Retry(key, retry) if key == "retry-0" && retry.attempt == 2 && retry.limit == Some(5) && retry.error.as_deref().filter(|value| !value.trim().is_empty()) == expected)
        );
        assert!(
            matches!(&blocks[2], Block::Retry(key, retry) if key == "retry-3" && retry.attempt == 1 && retry.limit == Some(5) && retry.error.is_none())
        );
        assert_eq!(
            history.snapshot.as_ref().unwrap().page.entries[0].parts,
            parts
        );
    }
}

#[test]
fn separates_counter_and_transport_resets() {
    let turn = TurnId::new();
    let parts = [
        json!({"type":"model_retry","attempt":1,"limit":5,"transport":"http","error":"First"}),
        json!({"type":"model_retry","attempt":2,"limit":5,"transport":"http","error":"Second"}),
        json!({"type":"model_retry","attempt":0,"fallback":true,"limit":5,"transport":"http","error":"Fallback"}),
        json!({"type":"model_retry","attempt":1,"limit":5,"transport":"http","error":"HTTP failure"}),
        json!({"type":"model_retry","attempt":1,"transport":"http","delay_ms":5000,"error":"Disconnected"}),
        json!({"type":"model_retry","attempt":2,"transport":"http","delay_ms":10000,"error":"Still disconnected","status":503,"code":"connection_lost"}),
        json!({"type":"model_retry","attempt":1,"limit":5,"transport":"http","error":"Next generation"}),
    ].into_iter().map(Part::Resource).collect::<Vec<_>>();
    let history = history(turn, parts.clone());
    let blocks = collect(turn, &history);
    assert_eq!(blocks.len(), 5);
    let expected = [
        ("retry-0", 2, Some(5), "Second"),
        ("retry-2", 0, Some(5), "Fallback"),
        ("retry-3", 1, Some(5), "HTTP failure"),
        ("retry-4", 2, None, "Still disconnected"),
        ("retry-6", 1, Some(5), "Next generation"),
    ];
    for (block, (key, attempt, limit, error)) in blocks.iter().zip(expected) {
        let Block::Retry(actual, retry) = block else {
            panic!("retry expected")
        };
        assert_eq!(actual, key);
        assert_eq!(retry.attempt, attempt);
        assert_eq!(retry.limit, limit);
        assert_eq!(retry.error.as_deref(), Some(error));
    }
    let Block::Retry(_, retry) = &blocks[3] else {
        unreachable!()
    };
    assert_eq!(retry.delay_ms, Some(10000));
    assert_eq!(retry.status, Some(503));
    assert_eq!(retry.code.as_deref(), Some("connection_lost"));
    assert_eq!(
        history.snapshot.as_ref().unwrap().page.entries[0].parts,
        parts
    );
}

fn history(turn: TurnId, parts: Vec<Part>) -> View {
    View {
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
                    parts,
                    usage: None,
                    citations: vec![],
                    search_suggestions: None,
                }],
            }),
        })),
        ..Default::default()
    }
}
