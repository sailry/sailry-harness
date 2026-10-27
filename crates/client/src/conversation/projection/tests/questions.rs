use super::*;
use conversation::question::{Answer, Input, Question, Spec, State};

fn question(run: &Run) -> Question {
    Question {
        id: QuestionId::new(),
        session: run.session,
        turn: run.turn,
        entry: "canonical-question".into(),
        index: 0,
        state: State::Pending,
    }
}

#[test]
fn recovers_answers() {
    let (mut projection, snapshot, run, _) = fixture();
    let question = question(&run);
    projection
        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    let update = frame(&snapshot, 11, Change::Question(question.clone()));
    assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
    assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
    let mut answered = question.clone();
    answered.state = State::Answered(Answer::Text(" 中文 🙂 ".into()));
    assert_eq!(
        projection
            .apply(1, frame(&snapshot, 13, Change::Question(answered.clone())))
            .unwrap(),
        Apply::Recover
    );
    assert_eq!(
        projection.snapshot().unwrap().page.questions,
        vec![question.clone()]
    );
    projection.reconnect(2).unwrap();
    let mut recovered = snapshot.clone();
    Arc::make_mut(&mut recovered.page)
        .questions
        .push(answered.clone());
    recovered.sequence = 1;
    projection
        .apply(2, Update::ConversationSnapshot(Box::new(recovered)))
        .unwrap();
    for changed in [
        question,
        Question {
            state: State::Cancelled,
            ..answered.clone()
        },
        Question {
            session: SessionId::new(),
            ..answered.clone()
        },
        Question {
            index: 1,
            ..answered.clone()
        },
        Question {
            state: State::Answered(Answer::Text("changed".into())),
            ..answered.clone()
        },
    ] {
        assert!(
            projection
                .apply(2, frame(&snapshot, 2, Change::Question(changed)))
                .is_err()
        );
    }
    assert_eq!(
        projection.snapshot().unwrap().page.questions,
        vec![answered]
    );
}

#[test]
fn rejects_inconsistent_questions() {
    let (mut projection, mut snapshot, run, _) = fixture();
    let question = question(&run);
    Arc::make_mut(&mut snapshot.page).entries.push(Entry {
        sequence: 1,
        id: question.entry.clone(),
        turn: run.turn,
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 1,
        usage: None,
        citations: vec![],
        search_suggestions: None,
        commentary: Default::default(),
        parts: vec![Part::ToolCall {
            presentation: Default::default(),
            grouping: Default::default(),
            id: Some("call".into()),
            name: "ask_user".into(),
            summary: Box::new(sailry_protocol::tool::detail::Summary {
                question: Some(Box::new(Spec {
                    prompt: "Pick".into(),
                    input: Input::Choice {
                        multiple: false,
                        allow_other: false,
                        options: vec!["one".into(), "two".into()],
                    },
                })),
                ..Default::default()
            }),
        }],
    });
    projection
        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    for changed in [
        Question {
            state: State::Declined,
            ..question.clone()
        },
        Question {
            turn: TurnId::new(),
            ..question.clone()
        },
        Question {
            index: 1,
            ..question.clone()
        },
        Question {
            state: State::Answered(Answer::Text("wrong type".into())),
            ..question.clone()
        },
    ] {
        assert!(
            projection
                .apply(1, frame(&snapshot, 11, Change::Question(changed)))
                .is_err()
        );
    }
    projection
        .apply(1, frame(&snapshot, 11, Change::Question(question.clone())))
        .unwrap();
    let repeated = Question {
        id: QuestionId::new(),
        ..question.clone()
    };
    assert!(
        projection
            .apply(1, frame(&snapshot, 12, Change::Question(repeated.clone())))
            .is_err()
    );
    Arc::make_mut(&mut snapshot.page).questions = vec![question, repeated];
    projection.reconnect(2).unwrap();
    assert!(
        projection
            .apply(2, Update::ConversationSnapshot(Box::new(snapshot)))
            .is_err()
    );
}

#[test]
fn validates_plan_answers() {
    for kind in ["plan", "text"] {
        let (mut projection, mut snapshot, run, _) = fixture();
        let mut question = question(&run);
        let input = if kind == "plan" {
            Input::Plan
        } else {
            Input::Text {
                multiline: true,
                max_bytes: 64,
            }
        };
        Arc::make_mut(&mut snapshot.page).entries.push(Entry {
            sequence: 1,
            id: question.entry.clone(),
            turn: run.turn,
            author: "assistant".into(),
            branch: String::new(),
            timestamp_ms: 1,
            usage: None,
            citations: vec![],
            search_suggestions: None,
            commentary: Default::default(),
            parts: vec![Part::ToolCall {
                presentation: Default::default(),
                grouping: Default::default(),
                id: Some("call".into()),
                name: "ask_user".into(),
                summary: Box::new(sailry_protocol::tool::detail::Summary {
                    question: Some(Box::new(Spec {
                        prompt: "Review".into(),
                        input,
                    })),
                    ..Default::default()
                }),
            }],
        });
        projection
            .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
            .unwrap();
        question.state = State::Answered(Answer::Plan { turn: run.turn });
        assert!(
            projection
                .apply(1, frame(&snapshot, 11, Change::Question(question.clone())))
                .is_err()
        );
        question.state = State::Answered(Answer::Plan {
            turn: TurnId::new(),
        });
        let result = projection.apply(1, frame(&snapshot, 11, Change::Question(question.clone())));
        if kind == "plan" {
            assert_eq!(result.unwrap(), Apply::Applied);
            Arc::make_mut(&mut snapshot.page).questions.push(question);
            snapshot.sequence = 11;
            projection.reconnect(2).unwrap();
            projection
                .apply(2, Update::ConversationSnapshot(Box::new(snapshot)))
                .unwrap();
        } else {
            assert!(result.is_err());
        }
    }
}

fn review_entry(question: &Question, spec: Spec) -> Entry {
    Entry {
        sequence: 1,
        id: question.entry.clone(),
        turn: question.turn,
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 1,
        usage: None,
        citations: vec![],
        search_suggestions: None,
        commentary: Default::default(),
        parts: vec![Part::ToolCall {
            presentation: Default::default(),
            grouping: Default::default(),
            id: Some("review".into()),
            name: "plan_review".into(),
            summary: Box::new(sailry_protocol::tool::detail::Summary {
                question: Some(Box::new(spec)),
                ..Default::default()
            }),
        }],
    }
}

#[test]
fn projects_automatic_reviews() {
    for state in [
        State::Pending,
        State::Answered(Answer::Plan {
            turn: TurnId::new(),
        }),
        State::Answered(Answer::Text(String::new())),
    ] {
        let (mut projection, mut snapshot, run, _) = fixture();
        let question = Question {
            state,
            ..question(&run)
        };
        Arc::make_mut(&mut snapshot.page).entries.push(review_entry(
            &question,
            Spec {
                prompt: String::new(),
                input: Input::Plan,
            },
        ));
        projection
            .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
            .unwrap();
        assert_eq!(
            projection
                .apply(1, frame(&snapshot, 11, Change::Question(question.clone())))
                .unwrap(),
            Apply::Applied
        );
        assert_eq!(
            projection.snapshot().unwrap().page.questions,
            vec![question.clone()]
        );
        Arc::make_mut(&mut snapshot.page).questions.push(question);
        snapshot.sequence = 11;
        projection.reconnect(2).unwrap();
        assert_eq!(
            projection
                .apply(2, Update::ConversationSnapshot(Box::new(snapshot.clone())))
                .unwrap(),
            Apply::Applied
        );
        assert_eq!(projection.snapshot().unwrap().page, snapshot.page);
    }
}

#[test]
fn recovers_completed_pending_review() {
    let (mut projection, snapshot, mut run, _) = fixture();
    let mut question = question(&run);
    let mut entry = review_entry(
        &question,
        Spec {
            prompt: String::new(),
            input: Input::Plan,
        },
    );
    entry.author = "plan_review".into();
    projection
        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    run.status = Status::Completed;
    run.finished_ms = Some(2000);
    for (sequence, change) in [
        (11, Change::Run(run)),
        (12, Change::Entry(entry)),
        (13, Change::Question(question.clone())),
    ] {
        let update = frame(&snapshot, sequence, change);
        assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
        assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
    }
    let recovered = projection.snapshot().unwrap().clone();
    let calls = crate::conversation::tools::collect(&recovered.page);
    assert_eq!(calls[0].question_pending(&recovered.page), Some(&question));
    assert_eq!(recovered.page.runs[0].status, Status::Completed);
    projection.reconnect(2).unwrap();
    projection
        .apply(2, Update::ConversationSnapshot(Box::new(recovered.clone())))
        .unwrap();
    assert_eq!(projection.snapshot().unwrap(), &recovered);
    question.state = State::Cancelled;
    let update = frame(&recovered, 14, Change::Question(question));
    assert_eq!(projection.apply(2, update.clone()).unwrap(), Apply::Applied);
    assert_eq!(projection.apply(2, update).unwrap(), Apply::Ignored);
    let page = &projection.snapshot().unwrap().page;
    assert!(
        crate::conversation::tools::collect(page)[0]
            .question_pending(page)
            .is_none()
    );
}

#[test]
fn rejects_invalid_review_sources() {
    for spec in [
        Spec {
            prompt: String::new(),
            input: Input::Text {
                multiline: true,
                max_bytes: 64,
            },
        },
        Spec {
            prompt: "Review".into(),
            input: Input::Text {
                multiline: true,
                max_bytes: 64,
            },
        },
        Spec {
            prompt: "Review".into(),
            input: Input::Plan,
        },
        Spec {
            prompt: " ".into(),
            input: Input::Plan,
        },
    ] {
        let (mut projection, mut snapshot, run, _) = fixture();
        let question = question(&run);
        Arc::make_mut(&mut snapshot.page)
            .entries
            .push(review_entry(&question, spec));
        projection
            .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
            .unwrap();
        assert!(
            projection
                .apply(1, frame(&snapshot, 11, Change::Question(question.clone())))
                .is_err()
        );
        assert!(projection.snapshot().unwrap().page.questions.is_empty());
        Arc::make_mut(&mut snapshot.page).questions.push(question);
        projection.reconnect(2).unwrap();
        assert!(
            projection
                .apply(2, Update::ConversationSnapshot(Box::new(snapshot)))
                .is_err()
        );
    }
}

#[test]
fn retains_nonempty_model_prompts() {
    let (mut projection, mut snapshot, run, _) = fixture();
    let question = question(&run);
    let mut entry = review_entry(
        &question,
        Spec {
            prompt: String::new(),
            input: Input::Plan,
        },
    );
    let Part::ToolCall { name, .. } = &mut entry.parts[0] else {
        panic!("tool call expected")
    };
    *name = "ask_user".into();
    Arc::make_mut(&mut snapshot.page).entries.push(entry);
    projection
        .apply(1, Update::ConversationSnapshot(Box::new(snapshot.clone())))
        .unwrap();
    assert!(
        projection
            .apply(1, frame(&snapshot, 11, Change::Question(question.clone())))
            .is_err()
    );
    assert!(projection.snapshot().unwrap().page.questions.is_empty());
    Arc::make_mut(&mut snapshot.page).questions.push(question);
    projection.reconnect(2).unwrap();
    assert!(
        projection
            .apply(2, Update::ConversationSnapshot(Box::new(snapshot)))
            .is_err()
    );
}
