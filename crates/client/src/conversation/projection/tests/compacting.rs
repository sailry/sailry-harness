use super::*;
use sailry_protocol::conversation::compaction::Progress;

fn entry(sequence: u64, id: &str, run: &Run, parts: Vec<Part>) -> Entry {
    Entry {
        sequence,
        id: id.into(),
        turn: run.turn,
        author: "sailry".into(),
        branch: String::new(),
        timestamp_ms: 1,
        parts,
        citations: vec![],
        search_suggestions: None,
        usage: None,
    }
}

fn progress() -> Part {
    Part::Resource(Progress { started_ms: 1_500 }.resource())
}

#[test]
fn validates_progress_and_terminal_outcomes() {
    for (case_name, run) in [
        (
            "follows_finished_tools_until_summary",
            (|case_name| {
                let (mut projection, snapshot, run, mut draft) = fixture();
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let result = Part::ToolResult {
                    id: Some("read".into()),
                    name: "read_file".into(),
                    summary: Default::default(),
                    images: vec![],
                };
                projection
                    .apply(
                        1,
                        frame(
                            &snapshot,
                            11,
                            Change::Entry(entry(41, "tool", &run, vec![result])),
                        ),
                    )
                    .unwrap();
                draft.id = "summary".into();
                draft.parts = vec![progress()];
                projection
                    .apply(1, frame(&snapshot, 12, Change::Delta(draft.clone())))
                    .unwrap();
                let view = projection.snapshot().unwrap();
                assert_eq!(view.drafts, vec![draft], "{case_name}");
                let rows = crate::conversation::rows::collect(view, &[], run.turn);
                let Part::Resource(value) = rows.last().unwrap().part else {
                    panic!("compacting row expected")
                };
                assert_eq!(
                    Progress::from_resource(value),
                    Some(Progress { started_ms: 1_500 }),
                    "{case_name}"
                );
                let summary = entry(
                    42,
                    "summary",
                    &run,
                    vec![Part::Compaction("Handoff".into())],
                );
                projection
                    .apply(1, frame(&snapshot, 13, Change::Entry(summary)))
                    .unwrap();
                assert!(
                    projection.snapshot().unwrap().drafts.is_empty(),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "clears_on_failure",
            (|case_name| {
                let (mut projection, snapshot, mut run, mut draft) = fixture();
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                draft.parts = vec![progress()];
                projection
                    .apply(1, frame(&snapshot, 11, Change::Delta(draft)))
                    .unwrap();
                run.status = Status::Failed;
                projection
                    .apply(1, frame(&snapshot, 12, Change::Run(run)))
                    .unwrap();
                assert!(
                    projection.snapshot().unwrap().drafts.is_empty(),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "rejects_other_resources",
            (|case_name| {
                let (mut projection, snapshot, _, mut draft) = fixture();
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                draft.parts = vec![Part::Resource(serde_json::json!({"type":"tool_started"}))];
                assert!(
                    projection
                        .apply(1, frame(&snapshot, 11, Change::Delta(draft)))
                        .is_err(),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
