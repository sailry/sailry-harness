use super::*;
use sailry_protocol::{NodeId, SessionId, conversation::Page};
use serde_json::json;
use std::sync::Arc;

#[test]
fn preserves_sources_across_streaming_and_visibility() {
    for (case_name, run) in [
        (
            "retains_sources_and_scalar_offsets",
            (|case_name| {
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
                assert_eq!(rows.len(), 5, "{case_name}");
                assert_eq!(
                    projected[0]["part"],
                    json!({"kind":"text","data":"A🙂中"}),
                    "{case_name}"
                );
                assert_eq!(projected[0]["source"]["index"], 0, "{case_name}");
                assert_eq!(projected[0]["end"], 2, "{case_name}");
                assert!(!rows[0].text_last, "{case_name}");
                assert_eq!(
                    projected[1]["part"],
                    json!({"kind":"thinking","data":"First thought"}),
                    "{case_name}"
                );
                assert_eq!(
                    projected[2]["part"],
                    json!({"kind":"text","data":"Tail"}),
                    "{case_name}"
                );
                assert_eq!(rows[2].text_offset, 3, "{case_name}");
                assert!(rows[2].text_last, "{case_name}");
                assert_eq!(projected[3]["source"]["index"], 6, "{case_name}");
                assert_eq!(projected[3]["part"]["data"]["attempt"], 2, "{case_name}");
                assert_eq!(
                    projected[4]["part"]["data"]["fallback"], true,
                    "{case_name}"
                );
                assert_eq!(snapshot.page.entries[0].parts, parts, "{case_name}");
            }) as fn(&str),
        ),
        (
            "isolates_streaming_changes",
            (|case_name| {
                let turn = TurnId::new();
                let other = TurnId::new();
                let before = snapshot(turn, vec![Part::Text("History".into())]);
                let mut after = before.clone();
                after.drafts.push(sailry_protocol::conversation::Draft {
                    id: "draft".into(),
                    turn: other,
                    author: "assistant".into(),
                    branch: String::new(),
                    parts: vec![Part::Text("Streaming".into())],
                });
                assert_eq!(
                    changed(&before, &after),
                    Some([other].into()),
                    "{case_name}"
                );
                assert_eq!(
                    changed(&before, &before),
                    Some(Default::default()),
                    "{case_name}"
                );
                assert_eq!(
                    changed(&after, &before),
                    Some([other].into()),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "tracks_entry_visibility",
            (|case_name| {
                let turn = TurnId::new();
                let before = snapshot(turn, vec![Part::Text("History".into())]);
                let mut after = before.clone();
                Arc::make_mut(&mut Arc::make_mut(&mut after.page).entries).clear();
                assert_eq!(changed(&before, &after), Some([turn].into()), "{case_name}");
                Arc::make_mut(&mut after.page).revision += 1;
                assert_eq!(changed(&before, &after), None, "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
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
            }]
            .into(),
        }),
    }
}
