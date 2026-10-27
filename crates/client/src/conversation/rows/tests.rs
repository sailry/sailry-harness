use super::*;
use sailry_protocol::{NodeId, SessionId, conversation::Page};
use serde_json::json;
use std::sync::Arc;

#[test]
fn retains_sources_and_scalar_offsets() {
    let turn = TurnId::new();
    let parts = vec![
        Part::Text("A🙂".into()),
        Part::Text("中".into()),
        Part::Thinking("First ".into()),
        Part::Thinking("thought".into()),
        Part::Text("Tail".into()),
        Part::Resource(json!({"type":"provider_context"})),
        Part::Resource(json!({"type":"model_retry","attempt":1,"limit":5})),
        Part::Resource(json!({"type":"model_retry","attempt":2,"limit":5})),
        Part::Resource(
            json!({"type":"model_retry","attempt":0,"limit":5,"fallback":true,"transport":"http"}),
        ),
    ];
    let snapshot = snapshot(turn, parts.clone());
    let rows = collect(&snapshot, &[], turn);
    let projected = serde_json::to_value(&rows).unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(projected[0]["part"], json!({"kind":"text","data":"A🙂中"}));
    assert_eq!(projected[0]["source"]["index"], 0);
    assert_eq!(projected[0]["end"], 2);
    assert!(!rows[0].text_last);
    assert_eq!(
        projected[1]["part"],
        json!({"kind":"thinking","data":"First thought"})
    );
    assert_eq!(projected[2]["part"], json!({"kind":"text","data":"Tail"}));
    assert_eq!(rows[2].text_offset, 3);
    assert!(rows[2].text_last);
    assert_eq!(projected[3]["source"]["index"], 6);
    assert_eq!(projected[3]["part"]["data"]["attempt"], 2);
    assert_eq!(projected[4]["part"]["data"]["fallback"], true);
    assert_eq!(snapshot.page.entries[0].parts, parts);
}

fn snapshot(turn: TurnId, parts: Vec<Part>) -> Snapshot {
    Snapshot {
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
                id: "entry".into(),
                turn,
                sequence: 1,
                author: "assistant".into(),
                branch: String::new(),
                timestamp_ms: 0,
                parts,
                usage: None,
                citations: vec![],
                search_suggestions: Some("Search suggestion markup".into()),
                commentary: Default::default(),
            }],
        }),
    }
}

#[test]
fn keeps_phase_boundaries_after_rebuild() {
    let turn = TurnId::new();
    let mut snapshot = snapshot(
        turn,
        vec![
            Part::Text("Checking ".into()),
            Part::Text("the file.".into()),
            Part::Text("Here is the answer".into()),
        ],
    );
    Arc::make_mut(&mut snapshot.page).entries[0].commentary =
        [(0, "msg-progress".into()), (1, "msg-progress".into())].into();
    let text = |snapshot: &Snapshot| {
        collect(snapshot, &[], turn)
            .into_iter()
            .map(|row| row.text.unwrap())
            .collect::<Vec<_>>()
    };
    let rows = collect(&snapshot, &[], turn);
    assert!(!rows[0].text_last);
    assert_eq!(rows[1].text_offset, 0);
    assert!(rows[1].text_last);
    let committed = text(&snapshot);
    assert_eq!(committed, ["Checking the file.", "Here is the answer"]);
    let entry = Arc::make_mut(&mut snapshot.page).entries.pop().unwrap();
    snapshot.drafts.push(sailry_protocol::conversation::Draft {
        id: entry.id,
        turn,
        author: entry.author,
        branch: entry.branch,
        parts: entry.parts,
        commentary: entry.commentary,
    });
    assert_eq!(text(&snapshot), committed);
}
