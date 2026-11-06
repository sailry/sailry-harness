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
fn validates_history_boundaries_and_watermarks() {
    for (case_name, run) in [
        (
            "recovers_structural_changes",
            (|case_name| {
                let (mut projection, snapshot, _, _) = fixture();
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let mut history = History {
                    sequence: snapshot.sequence,
                    page: (*snapshot.page).clone(),
                    missing: vec![],
                };
                history.page.revision += 1;
                assert_eq!(
                    projection.merge_history(1, None, history.clone()).unwrap(),
                    Apply::Recover,
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap(), &snapshot, "{case_name}");
                let mut current = (*snapshot.page).clone();
                assert_eq!(
                    super::super::super::history::merge(&mut current, history.page.clone(), false)
                        .unwrap_err()
                        .code,
                    ErrorCode::RevisionConflict,
                    "{case_name}"
                );
                assert_eq!(current, *snapshot.page, "{case_name}");
                projection.reconnect(2).unwrap();
                let mut fresh = snapshot.clone();
                fresh.page = Arc::new(history.page);
                projection
                    .apply(2, Update::ConversationSnapshot(Box::new(fresh.clone())))
                    .unwrap();
                let stale = History {
                    sequence: snapshot.sequence,
                    page: (*snapshot.page).clone(),
                    missing: vec![],
                };
                assert_eq!(
                    projection.merge_history(1, None, stale).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap(), &fresh, "{case_name}");
            }) as fn(&str),
        ),
        (
            "merges_turns_after_watermark",
            (|case_name| {
                let (mut projection, mut snapshot, mut run, draft) = fixture();
                run.sequence = 2;
                let page = Arc::make_mut(&mut snapshot.page);
                page.runs = vec![run.clone()];
                page.entries = vec![entry(&run, 3)].into();
                page.next_before = Some(run.turn);
                snapshot.drafts.push(draft);
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                assert_eq!(projection.live_start(), None, "{case_name}");
                let older = Run {
                    turn: TurnId::new(),
                    sequence: 1,
                    status: Status::Completed,
                    ..run.clone()
                };
                let history = History {
                    sequence: 11,
                    missing: vec![],
                    page: Page {
                        session: run.session,
                        revision: 1,
                        runs: vec![older.clone()],
                        entries: vec![entry(&older, 1), entry(&older, 2)].into(),
                        approvals: vec![],
                        questions: vec![],
                        children: Vec::new(),
                        background: Vec::new(),
                        queue: Default::default(),
                        next_before: None,
                    },
                };
                assert_eq!(
                    projection
                        .merge_history(1, Some(run.turn), history.clone())
                        .unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap(), &snapshot, "{case_name}");
                let mut paged = Projection::new(snapshot.node, run.session, 1);
                paged
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let mut older_page = history.clone();
                older_page.sequence = snapshot.sequence;
                older_page.missing = vec![older.turn];
                assert_eq!(
                    paged.merge_history(1, Some(run.turn), older_page).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(paged.live_start(), None, "{case_name}");
                assert_eq!(
                    paged.snapshot().unwrap().missing,
                    [older.turn],
                    "{case_name}"
                );
                assert_eq!(
                    paged.snapshot().unwrap().page.entries.len(),
                    3,
                    "{case_name}"
                );
                projection
                    .apply(1, frame(&snapshot, 11, Change::Entry(entry(&run, 4))))
                    .unwrap();
                let before = projection.snapshot().unwrap().clone();
                let mut conflicting = history.clone();
                Arc::make_mut(&mut conflicting.page.entries)[0].id =
                    before.page.entries[0].id.clone();
                assert!(
                    projection
                        .merge_history(1, Some(run.turn), conflicting)
                        .is_err(),
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap(), &before, "{case_name}");
                assert_eq!(
                    projection
                        .merge_history(1, Some(run.turn), history.clone())
                        .unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                let complete = projection.snapshot().unwrap().clone();
                assert_eq!(complete.page.entries.len(), 4, "{case_name}");
                assert_eq!(complete.page.runs, [older, run.clone()], "{case_name}");
                assert_eq!(complete.page.next_before, None, "{case_name}");
                assert_eq!(complete.sequence, 11, "{case_name}");
                assert_eq!(complete.drafts, snapshot.drafts, "{case_name}");
                assert_eq!(
                    projection
                        .merge_history(1, Some(run.turn), history.clone())
                        .unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(
                    projection.merge_history(0, None, history).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap(), &complete, "{case_name}");
                assert_eq!(projection.live_start(), Some(4), "{case_name}");
            }) as fn(&str),
        ),
        (
            "rejects_invalid_boundaries",
            (|case_name| {
                let (mut projection, mut snapshot, mut run, _) = fixture();
                run.sequence = 2;
                let page = Arc::make_mut(&mut snapshot.page);
                page.runs = vec![run.clone()];
                page.entries = vec![entry(&run, 2)].into();
                page.next_before = Some(run.turn);
                projection
                    .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                    .unwrap();
                let history = History {
                    sequence: 10,
                    page: (*snapshot.page).clone(),
                    missing: vec![],
                };
                assert!(
                    projection
                        .merge_history(1, Some(run.turn), history.clone())
                        .is_err(),
                    "{case_name}"
                );
                for index in 0..6 {
                    let mut changed = history.clone();
                    match index {
                        0 => std::sync::Arc::make_mut(&mut changed.page.entries)[0].sequence = 1,
                        1 => {
                            std::sync::Arc::make_mut(&mut changed.page.entries)[0].parts =
                                vec![Part::Text("changed".into())]
                        }
                        2 => changed.page.runs[0].revision += 1,
                        3 => changed.page.runs[0].sequence += 1,
                        4 => changed.page.runs[0].kind = conversation::RunKind::Compaction,
                        _ => changed.page.session = SessionId::new(),
                    }
                    assert!(
                        projection.merge_history(1, None, changed).is_err(),
                        "{case_name}"
                    );
                    assert_eq!(projection.snapshot().unwrap(), &snapshot, "{case_name}");
                }
                let mut duplicate = (*snapshot.page).clone();
                let entry = duplicate.entries[0].clone();
                std::sync::Arc::make_mut(&mut duplicate.entries).push(entry);
                assert!(validate_page(&duplicate).is_err(), "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
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
        access: None,
        startup: None,
        ssh: None,
        changes: None,
        can_remember: true,
        prefix: Some("cargo test".into()),
        choice: Some(conversation::ApprovalChoice::Session),
        reason: None,
    });
    let original = page.clone();
    let mut stale = page.clone();
    stale.questions[0].state = State::Pending;
    stale.approvals[0].state = ApprovalState::Pending;
    stale.approvals[0].choice = None;
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
    for field in 0..6 {
        let mut changed = original.clone();
        match field {
            0 => changed.approvals[0].choice = Some(conversation::ApprovalChoice::Once),
            1 => changed.approvals[0].reason = Some("Changed constraint".into()),
            2 => changed.approvals[0].prefix = Some("cargo build".into()),
            3 => changed.approvals[0].can_remember = false,
            4 => changed.approvals[0].source = ApprovalSource::SessionRule,
            _ => changed.approvals[0].access = Some(sailry_protocol::process::Access::Unrestricted),
        }
        assert!(crate::conversation::history::merge(&mut page, changed, true).is_err());
        assert_eq!(page, original);
    }
}

#[test]
fn recovers_diagnostics_and_continued_turn_visibility() {
    for (case_name, run) in [
        (
            "recovers_compaction_failures_without_duplicates",
            (|case_name| {
                use conversation::compaction::{Failure, Reason, Source, Stage};
                for source in [Source::Automatic, Source::Manual] {
                    let (mut projection, snapshot, run, _) = fixture();
                    projection
                        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                        .unwrap();
                    let mut failure = entry(&run, 41);
                    failure.author = "system".into();
                    failure.parts = vec![Part::Resource(
                        Failure {
                            source,
                            stage: Stage::Request,
                            reason: Reason::RateLimited,
                            continued: source == Source::Automatic,
                            status: Some(429),
                            cause: Some("Rate limit reached".into()),
                        }
                        .resource(),
                    )];
                    let update = frame(&snapshot, 11, Change::Entry(failure.clone()));
                    assert_eq!(
                        projection.apply(1, update.clone()).unwrap(),
                        Apply::Applied,
                        "{case_name}"
                    );
                    assert_eq!(
                        projection.apply(1, update.clone()).unwrap(),
                        Apply::Ignored,
                        "{case_name}"
                    );
                    let saved = projection.snapshot().unwrap().clone();
                    projection.reconnect(2).unwrap();
                    assert_eq!(
                        projection.apply(1, update).unwrap(),
                        Apply::Ignored,
                        "{case_name}"
                    );
                    projection
                        .apply(2, Update::ConversationSnapshot(Box::new(saved)))
                        .unwrap();
                    assert_eq!(
                        projection.snapshot().unwrap().page.entries.as_slice(),
                        &[failure],
                        "{case_name}"
                    );
                }
            }) as fn(&str),
        ),
        (
            "preserves_recovery_diagnostics_across_reconnect",
            (|case_name| {
                use conversation::recovery::{Retry, Transport};
                for (attempt, limit) in [(2, Some(5)), (9, None)] {
                    let (mut projection, mut snapshot, run, draft) = fixture();
                    snapshot.drafts.push(draft.clone());
                    projection
                        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                        .unwrap();
                    let mut retry = entry(&run, 41);
                    retry.author = "system".into();
                    retry.parts = vec![Part::Resource(
                        Retry {
                            attempt,
                            fallback: false,
                            limit,
                            delay_ms: Some(60_000),
                            transport: Some(Transport::Http),
                            status: Some(503),
                            code: Some("connection_failed".into()),
                            error: Some("Original connection failure".into()),
                        }
                        .resource(),
                    )];
                    let update = frame(&snapshot, 11, Change::Entry(retry.clone()));
                    assert_eq!(
                        projection.apply(1, update.clone()).unwrap(),
                        Apply::Applied,
                        "{case_name}"
                    );
                    assert_eq!(
                        projection.apply(1, update.clone()).unwrap(),
                        Apply::Ignored,
                        "{case_name}"
                    );
                    let saved = projection.snapshot().unwrap().clone();
                    projection.reconnect(2).unwrap();
                    assert_eq!(
                        projection.apply(1, update).unwrap(),
                        Apply::Ignored,
                        "{case_name}"
                    );
                    projection
                        .apply(2, Update::ConversationSnapshot(Box::new(saved)))
                        .unwrap();
                    let current = projection.snapshot().unwrap();
                    assert_eq!(current.page.entries.as_slice(), &[retry], "{case_name}");
                    assert_eq!(current.page.runs, vec![run], "{case_name}");
                    assert_eq!(current.drafts, vec![draft], "{case_name}");
                }
            }) as fn(&str),
        ),
        (
            "keeps_failures_visible_after_guidance",
            (|case_name| {
                use conversation::compaction::{Failure, Reason, Source, Stage};
                let (_, mut snapshot, mut run, _) = fixture();
                run.status = Status::Continued;
                run.continuation = Some(conversation::Continuation {
                    package: sailry_protocol::plugin::Reference {
                        name: "fixture".into(),
                        digest: "fixture".into(),
                        settings_revision: 1,
                    },
                    marker: None,
                });
                let mut entry = entry(&run, 41);
                entry.author = "system".into();
                entry.parts = vec![Part::Resource(
                    Failure {
                        source: Source::Automatic,
                        stage: Stage::History,
                        reason: Reason::HistoryMismatch,
                        continued: true,
                        status: None,
                        cause: None,
                    }
                    .resource(),
                )];
                Arc::make_mut(&mut Arc::make_mut(&mut snapshot.page).entries).push(entry);
                assert!(
                    crate::conversation::turns::visible(&run, &snapshot.page, &[]),
                    "{case_name}"
                );
                Arc::make_mut(&mut Arc::make_mut(&mut snapshot.page).entries)[0].parts =
                    vec![Part::Resource(
                        serde_json::json!({"type":"provider_context"}),
                    )];
                assert!(
                    !crate::conversation::turns::visible(&run, &snapshot.page, &[]),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
