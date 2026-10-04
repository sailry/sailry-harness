use super::*;
use serde_json::json;

#[test]
fn updates_preserve_failures() {
    let turn = TurnId::new();
    let other = TurnId::new();
    let mut entries = Vec::new();
    for (index, turn, state) in [
        (0, turn, "pending"),
        (1, turn, "completed"),
        (2, other, "in_progress"),
    ] {
        let args = json!({"title": null, "steps": [{"description": "Verify", "state": state}]});
        entries.push(Entry {
            sequence: index + 1,
            id: format!("entry-{index}"),
            turn,
            author: "assistant".into(),
            branch: String::new(),
            timestamp_ms: 1,
            usage: None,
            citations: vec![],
            search_suggestions: None,
            parts: vec![
                Part::ToolCall {
                    display: None,
                    presentation: sailry_protocol::tool::Presentation::Progress,
                    grouping: Default::default(),
                    id: Some(format!("call-{index}")),
                    name: "update_plan".into(),
                    arguments: args.clone(),
                },
                Part::ToolResult {
                    id: Some(format!("call-{index}")),
                    name: "update_plan".into(),
                    result: json!({"progress": args}),
                    images: vec![],
                },
                Part::Thinking(format!("reasoning-{index}")),
            ],
        });
    }
    let mut failed = entries[1].clone();
    failed.id = "failed".into();
    failed.sequence = 4;
    failed.parts = vec![
        Part::ToolCall {
            display: None,
            presentation: sailry_protocol::tool::Presentation::Progress,
            grouping: Default::default(),
            id: Some("bad".into()),
            name: "update_plan".into(),
            arguments: json!({}),
        },
        Part::ToolResult {
            id: Some("bad".into()),
            name: "update_plan".into(),
            result: json!({"error": "invalid plan"}),
            images: vec![],
        },
    ];
    entries.push(failed);
    let page = Page {
        session: SessionId::new(),
        revision: 1,
        next_before: None,
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        runs: vec![],
        entries,
    };
    let calls = page
        .entries
        .iter()
        .map(|entry| {
            let Part::ToolCall {
                id,
                name,
                arguments,
                ..
            } = &entry.parts[0]
            else {
                unreachable!()
            };
            Call {
                presentation: sailry_protocol::tool::Presentation::Progress,
                grouping: Default::default(),
                turn: entry.turn,
                id: id.clone(),
                name: name.clone(),
                source: Reference {
                    entry: entry.id.clone(),
                    index: 0,
                },
                response: Some(Reference {
                    entry: entry.id.clone(),
                    index: 1,
                }),
                progress: serde_json::from_value(arguments.clone()).ok(),
                content: None,
                resolved: None,
                state: State::Returned,
                approval: None,
                question: None,
            }
        })
        .collect();
    let history = View {
        calls: Arc::new(calls),
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([1; 32]),
            sequence: 1,
            missing: vec![],
            drafts: vec![],
            statistics: Default::default(),
            page: Arc::new(page),
        })),
        ..Default::default()
    };
    let original = history.calls.clone();
    let blocks = collect(turn, &history);
    let calls: Vec<_> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Tools(calls) => Some(calls),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id.as_deref(), Some("call-1"));
    assert_eq!(calls[1].id.as_deref(), Some("bad"));
    assert!(matches!(&blocks[0], Block::Tools(_)));
    assert!(matches!(&blocks[1], Block::Thinking(_, text) if text == "reasoning-0"));
    let blocks = collect(other, &history);
    assert!(
        matches!(&blocks[0], Block::Tools(calls) if calls.len() == 1 && calls[0].id.as_deref() == Some("call-2"))
    );
    assert_eq!(history.calls, original);
}
