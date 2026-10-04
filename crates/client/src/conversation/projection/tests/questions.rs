use super::*;
use conversation::question::{Answer, Question, State};

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
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
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
        .apply(2, Update::ConversationSnapshot(recovered))
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
        sequence: 1, id: question.entry.clone(), turn: run.turn, author: "assistant".into(), branch: String::new(), timestamp_ms: 1, usage: None, citations: vec![], search_suggestions: None,
        parts: vec![Part::ToolCall { display: None, presentation: Default::default(), grouping: Default::default(), id: Some("call".into()), name: "ask_user".into(), arguments: serde_json::json!({"prompt": "Pick", "input": {"kind": "choice", "multiple": false, "allow_other": false, "options": ["one", "two"]}}) }],
    });
    projection
        .apply(1, Update::ConversationSnapshot(snapshot.clone()))
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
            .apply(2, Update::ConversationSnapshot(snapshot))
            .is_err()
    );
}

#[test]
fn validates_plan_answers() {
    for kind in ["plan", "text"] {
        let (mut projection, mut snapshot, run, _) = fixture();
        let mut question = question(&run);
        let input = if kind == "plan" {
            serde_json::json!({"kind": "plan"})
        } else {
            serde_json::json!({"kind": "text", "multiline": true, "max_bytes": 64})
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
            parts: vec![Part::ToolCall {
                display: None,
                presentation: Default::default(),
                grouping: Default::default(),
                id: Some("call".into()),
                name: "ask_user".into(),
                arguments: serde_json::json!({"prompt": "Review", "input": input}),
            }],
        });
        projection
            .apply(1, Update::ConversationSnapshot(snapshot.clone()))
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
                .apply(2, Update::ConversationSnapshot(snapshot))
                .unwrap();
        } else {
            assert!(result.is_err());
        }
    }
}
