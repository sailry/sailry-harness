use super::*;
use sailry_protocol::conversation::compaction::{Failure, Reason, Source, Stage};

#[test]
fn retains_failure_identity_between_text() {
    for source in [Source::Automatic, Source::Manual] {
        let turn = TurnId::new();
        let failure = Failure {
            source,
            stage: Stage::History,
            reason: Reason::HistoryMismatch,
            continued: false,
            status: None,
            cause: None,
        };
        let entry = Entry {
            id: "diagnostic".into(),
            sequence: 7,
            turn,
            author: "system".into(),
            branch: String::new(),
            timestamp_ms: 1,
            parts: vec![
                Part::Text("Before".into()),
                Part::Resource(failure.resource()),
                Part::Text("After".into()),
            ],
            usage: None,
            citations: vec![],
            search_suggestions: None,
        };
        let history = View {
            snapshot: Some(Arc::new(Snapshot {
                node: NodeId([1; 32]),
                sequence: 7,
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
                    entries: vec![entry],
                }),
            })),
            ..Default::default()
        };
        let blocks = collect(turn, &history);
        assert_eq!(blocks.len(), 3);
        assert!(
            matches!(&blocks[1], Block::CompactionFailure(key, value) if key == "diagnostic-1" && value == &failure)
        );
    }
}
