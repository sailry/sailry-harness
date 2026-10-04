//! Score canonical Node history; existing workflow assertions establish the actual outcome.
//! Fixture and opt-in live cases report their model and resource coverage separately.
use adk_eval::{
    ToolTrajectoryConfig,
    schema::ToolUse,
    scoring::{ResponseScorer, ToolTrajectoryScorer},
};
use sailry_protocol::conversation::{Page, Part};
use serde_json::Value;

pub(super) fn trajectory(page: &Page, expected: &[(String, Value)]) {
    let actual: Vec<_> = page
        .entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::ToolCall {
                name, arguments, ..
            } => Some(ToolUse::new(name).with_args(arguments.clone())),
            _ => None,
        })
        .collect();
    let expected: Vec<_> = expected
        .iter()
        .map(|(name, args)| ToolUse::new(name).with_args(args.clone()))
        .collect();
    let score = ToolTrajectoryScorer::with_config(ToolTrajectoryConfig {
        strict_order: true,
        strict_args: true,
    })
    .score(&expected, &actual);
    assert_eq!(
        score, 1.,
        "tool trajectory mismatch: expected={expected:?}, actual={actual:?}"
    );
    // A perfect trajectory can contain denied or failed calls. Resource and
    // outcome assertions in each case remain independent of this score.
}

pub(super) fn response(page: &Page, expected: &str) {
    let actual = page
        .entries
        .iter()
        .filter(|entry| entry.author != "user")
        .map(|entry| {
            entry
                .parts
                .iter()
                .filter_map(|part| match part {
                    Part::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<String>()
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let score = ResponseScorer::new().score(expected, &actual);
    assert_eq!(score, 1., "response mismatch: {actual}");
}

#[test]
fn preserves_word_boundaries() {
    use sailry_protocol::{
        SessionId, TurnId,
        conversation::{Entry, Queue},
    };
    let mut entry = Entry {
        sequence: 1,
        id: "streamed-answer".into(),
        turn: TurnId::new(),
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 0,
        parts: "Complete answer"
            .chars()
            .map(|ch| Part::Text(ch.to_string()))
            .collect(),
        citations: vec![],
        search_suggestions: None,
        usage: None,
    };
    entry
        .parts
        .insert(4, Part::Thinking("Internal reasoning".into()));
    response(
        &Page {
            session: SessionId::new(),
            revision: 1,
            entries: vec![entry],
            runs: vec![],
            queue: Queue::default(),
            approvals: vec![],
            questions: vec![],
            children: vec![],
            next_before: None,
        },
        "Complete answer",
    );
}
