use super::*;
use conversation::{
    Approval, ApprovalSource, ApprovalState, History,
    question::{Answer, Question, State},
};

fn entry(run: &Run, sequence: u64) -> Entry {
    Entry {
        sequence,
        id: format!("event-{sequence}"),
        turn: run.turn,
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 1,
        parts: vec![Part::Text("fixture".into())],
        citations: vec![],
        search_suggestions: None,
        usage: None,
    }
}

#[test]
fn recovers_structural_changes() {
    let (mut projection, snapshot, _, _) = fixture();
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let mut history = History {
        sequence: snapshot.sequence,
        page: (*snapshot.page).clone(),
        missing: vec![],
    };
    history.page.revision += 1;
    assert_eq!(
        projection.merge_history(1, None, history.clone()).unwrap(),
        Apply::Recover
    );
    assert_eq!(projection.snapshot().unwrap(), &snapshot);
    let mut current = (*snapshot.page).clone();
    assert_eq!(
        super::super::super::history::merge(&mut current, history.page.clone(), false)
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(current, *snapshot.page);
    projection.reconnect(2).unwrap();
    let mut fresh = snapshot.clone();
    fresh.page = Arc::new(history.page);
    projection
        .apply(2, Update::ConversationSnapshot(fresh.clone()))
        .unwrap();
    let stale = History {
        sequence: snapshot.sequence,
        page: (*snapshot.page).clone(),
        missing: vec![],
    };
    assert_eq!(
        projection.merge_history(1, None, stale).unwrap(),
        Apply::Ignored
    );
    assert_eq!(projection.snapshot().unwrap(), &fresh);
}

#[test]
fn merges_turns_after_watermark() {
    let (mut projection, mut snapshot, mut run, draft) = fixture();
    run.sequence = 2;
    let page = Arc::make_mut(&mut snapshot.page);
    page.runs = vec![run.clone()];
    page.entries = vec![entry(&run, 3)];
    page.next_before = Some(run.turn);
    snapshot.drafts.push(draft);
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let older = Run {
        turn: TurnId::new(),
        sequence: 1,
        status: Status::Completed,
        ..run.clone()
    };
    let mut history = History {
        sequence: 11,
        missing: vec![],
        page: Page {
            session: run.session,
            revision: 1,
            runs: vec![older.clone()],
            entries: vec![entry(&older, 1), entry(&older, 2)],
            approvals: vec![],
            questions: vec![],
            children: Vec::new(),
            queue: Default::default(),
            next_before: None,
        },
    };
    assert_eq!(
        projection
            .merge_history(1, Some(run.turn), history.clone())
            .unwrap(),
        Apply::Ignored
    );
    assert_eq!(projection.snapshot().unwrap(), &snapshot);
    projection
        .apply(1, frame(&snapshot, 11, Change::Entry(entry(&run, 4))))
        .unwrap();
    let before = projection.snapshot().unwrap().clone();
    history.missing = vec![older.turn];
    assert!(
        projection
            .merge_history(1, Some(run.turn), history.clone())
            .is_err()
    );
    assert_eq!(projection.snapshot().unwrap(), &before);
    history.missing.clear();
    assert_eq!(
        projection
            .merge_history(1, Some(run.turn), history.clone())
            .unwrap(),
        Apply::Applied
    );
    let complete = projection.snapshot().unwrap().clone();
    assert_eq!(complete.page.entries.len(), 4);
    assert_eq!(complete.page.runs, [older, run.clone()]);
    assert_eq!(complete.page.next_before, None);
    assert_eq!(complete.sequence, 11);
    assert_eq!(complete.drafts, snapshot.drafts);
    assert_eq!(
        projection
            .merge_history(1, Some(run.turn), history.clone())
            .unwrap(),
        Apply::Ignored
    );
    assert_eq!(
        projection.merge_history(0, None, history).unwrap(),
        Apply::Ignored
    );
    assert_eq!(projection.snapshot().unwrap(), &complete);
}

#[test]
fn preserves_live_metadata() {
    let (_, snapshot, run, _) = fixture();
    let mut page = (*snapshot.page).clone();
    page.queue.revision = 3;
    page.queue.paused = true;
    page.runs[0].status = Status::Completed;
    page.questions.push(Question {
        id: QuestionId::new(),
        session: run.session,
        turn: run.turn,
        entry: "question-call".into(),
        index: 0,
        state: State::Answered(Answer::Text("原文 🙂".into())),
    });
    page.approvals.push(Approval {
        id: ApprovalId::new(),
        session: run.session,
        turn: run.turn,
        entry: "approval-call".into(),
        index: 0,
        state: ApprovalState::Approved,
        source: ApprovalSource::User,
    });
    let original = page.clone();
    let mut stale = page.clone();
    stale.questions[0].state = State::Pending;
    stale.approvals[0].state = ApprovalState::Pending;
    stale.runs[0].status = Status::Running;
    stale.queue = Default::default();
    crate::conversation::history::merge(&mut page, stale, true).unwrap();
    assert_eq!(page, original);
    for question in [
        State::Cancelled,
        State::Answered(Answer::Text("changed".into())),
    ] {
        let mut changed = original.clone();
        changed.questions[0].state = question;
        assert!(crate::conversation::history::merge(&mut page, changed, true).is_err());
        assert_eq!(page, original);
    }
    let mut changed = original.clone();
    changed.approvals[0].state = ApprovalState::Denied;
    assert!(crate::conversation::history::merge(&mut page, changed, true).is_err());
    assert_eq!(page, original);
}

#[test]
fn rejects_invalid_boundaries() {
    let (mut projection, mut snapshot, mut run, _) = fixture();
    run.sequence = 2;
    let page = Arc::make_mut(&mut snapshot.page);
    page.runs = vec![run.clone()];
    page.entries = vec![entry(&run, 2)];
    page.next_before = Some(run.turn);
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
        .unwrap();
    let history = History {
        sequence: 10,
        page: (*snapshot.page).clone(),
        missing: vec![],
    };
    assert!(
        projection
            .merge_history(1, Some(run.turn), history.clone())
            .is_err()
    );
    for index in 0..6 {
        let mut changed = history.clone();
        match index {
            0 => changed.page.entries[0].sequence = 1,
            1 => changed.page.entries[0].parts = vec![Part::Text("changed".into())],
            2 => changed.page.runs[0].revision += 1,
            3 => changed.page.runs[0].sequence += 1,
            4 => changed.page.runs[0].kind = conversation::RunKind::Compaction,
            _ => changed.page.session = SessionId::new(),
        }
        assert!(projection.merge_history(1, None, changed).is_err());
        assert_eq!(projection.snapshot().unwrap(), &snapshot);
    }
    let mut duplicate = (*snapshot.page).clone();
    duplicate.entries.push(duplicate.entries[0].clone());
    assert!(validate_page(&duplicate).is_err());
}
