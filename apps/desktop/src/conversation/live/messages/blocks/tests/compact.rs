use super::*;

fn call(index: usize) -> Call {
    Call {
        terminal_interaction: false,
        presentation: Default::default(),
        grouping: Default::default(),
        turn: TurnId::new(),
        id: Some(index.to_string()),
        name: "read_file".into(),
        progress: None,
        resolved: None,
        detail: None,
        source: Reference {
            entry: "entry".into(),
            index,
        },
        response: None,
        state: State::Running,
        failure: None,
        approval: None,
        question: None,
    }
}

#[test]
fn visible_boundaries() {
    for (scenario, check) in [
        (
            "joins_across_reasoning_in_order",
            (|_scenario: &str| {
                let calls = [call(0), call(1), call(2)];
                let blocks = super::super::compact(vec![
                    Block::Thinking("first".into(), "Before".into()),
                    Block::Tools(vec![&calls[0]]),
                    Block::Thinking("middle".into(), "Between".into()),
                    Block::Tools(vec![&calls[1], &calls[2]]),
                    Block::Thinking("last".into(), "After".into()),
                ]);
                assert_eq!(blocks.len(), 1, "joins across reasoning in order");
                let Block::Tools(group) = &blocks[0] else {
                    panic!("tool group expected")
                };
                assert_eq!(
                    group
                        .iter()
                        .map(|call| call.source.index)
                        .collect::<Vec<_>>(),
                    vec![0, 1, 2],
                    "joins across reasoning in order"
                );
                assert!(
                    group.iter().all(|call| call.state == State::Running),
                    "joins across reasoning in order"
                );
            }) as fn(&str),
        ),
        (
            "preserves_visible_boundaries",
            (|_scenario: &str| {
                let first = call(0);
                let second = call(1);
                let mut standalone = call(2);
                standalone.grouping = sailry_protocol::tool::Grouping::Standalone;
                let mut progress = call(3);
                progress.presentation = sailry_protocol::tool::Presentation::Progress;
                for separator in [
                    Block::Text("text".into(), "[Report](output/report.docx)".into()),
                    Block::Question(&first),
                    Block::Tools(vec![&standalone]),
                    Block::Tools(vec![&progress]),
                    Block::Children(vec![]),
                    Block::Resource,
                    Block::Compaction("summary".into(), "Summary"),
                    Block::Retry(
                        "retry".into(),
                        sailry_protocol::conversation::recovery::Retry::from_resource(
                            &serde_json::json!({"type":"model_retry","attempt":1,"limit":5}),
                        )
                        .unwrap(),
                    ),
                ] {
                    let blocks = super::super::compact(vec![
                        Block::Tools(vec![&first]),
                        Block::Thinking("before".into(), "Before".into()),
                        separator,
                        Block::Thinking("after".into(), "After".into()),
                        Block::Tools(vec![&second]),
                    ]);
                    assert_eq!(blocks.len(), 3, "preserves visible boundaries");
                    assert!(
                        matches!(&blocks[0], Block::Tools(group) if group.len() == 1 && std::ptr::eq(group[0], &first)),
                        "preserves visible boundaries"
                    );
                    assert!(
                        matches!(&blocks[2], Block::Tools(group) if group.len() == 1 && std::ptr::eq(group[0], &second)),
                        "preserves visible boundaries"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
