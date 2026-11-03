use super::*;
use sailry_protocol::conversation::{Entry, Page, Part, Run, RunKind, Status};

mod statistics;

fn run(session: SessionId, sequence: u64) -> Run {
    Run {
        worktree: sailry_protocol::WorktreeId::new(),
        kind: RunKind::Task,
        turn: TurnId::new(),
        session,
        sequence,
        revision: 1,
        status: Status::Completed,
        error: None,
        started_ms: Some(1),
        finished_ms: Some(2),
        origin: None,
        continuation: None,
        guidance: None,
        joined: None,
        notice: false,
        notices: Vec::new(),
        input: None,
    }
}

fn entry(run: &Run, sequence: u64) -> Entry {
    Entry {
        sequence,
        id: format!("entry-{sequence}"),
        turn: run.turn,
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 1,
        parts: vec![Part::Text("答复".into())],
        citations: Vec::new(),
        search_suggestions: None,
        usage: None,
    }
}

/// Two turns: the older one is partial from entry 5 and older turns remain unread.
fn partial_view() -> (Snapshot, Run, Run) {
    let session = SessionId::new();
    let older = run(session, 2);
    let newer = run(session, 3);
    let snapshot = Snapshot {
        node: NodeId([5; 32]),
        sequence: 1,
        page: Arc::new(Page {
            session,
            revision: 1,
            entries: vec![entry(&older, 5), entry(&older, 6), entry(&newer, 7)].into(),
            runs: vec![older.clone(), newer.clone()],
            next_before: Some(older.turn),
            queue: Default::default(),
            approvals: Vec::new(),
            questions: Vec::new(),
            children: Vec::new(),
            background: Vec::new(),
        }),
        missing: vec![older.turn],
        drafts: Vec::new(),
        statistics: Default::default(),
        artifacts: Default::default(),
    };
    (snapshot, older, newer)
}

mod extent {
    use super::*;

    #[test]
    fn recent_view_reads_partial_turns_on_request() {
        let (snapshot, older, _) = partial_view();
        assert!(!Extent::Recent.needs_more(&snapshot));
        assert_eq!(partial(&snapshot), Some(older.turn));
        assert_eq!(
            Extent::from(&snapshot),
            Extent::Partial {
                turn: 2,
                entry: Some(5)
            }
        );
    }

    #[test]
    fn restores_loaded_partial_range() {
        let (mut snapshot, older, newer) = partial_view();
        let extent = Extent::Partial {
            turn: 2,
            entry: Some(5),
        };
        assert!(!extent.needs_more(&snapshot));
        // After resubscription only the newest entries of the partial turn remain.
        Arc::make_mut(&mut snapshot.page).entries = vec![entry(&older, 6), entry(&newer, 7)].into();
        assert!(extent.needs_more(&snapshot));
        // A newer partial turn was complete before and must be completed again.
        let (mut snapshot, _, newer) = partial_view();
        snapshot.missing.push(newer.turn);
        assert!(extent.needs_more(&snapshot));
    }

    #[test]
    fn search_targets_complete_partial_turns() {
        let (snapshot, _, _) = partial_view();
        assert!(Extent::Through(2).needs_more(&snapshot));
        assert!(!Extent::Through(3).needs_more(&snapshot));
        assert!(Extent::All.needs_more(&snapshot));
    }
}
