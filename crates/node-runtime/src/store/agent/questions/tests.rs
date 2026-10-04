use super::super::tests::command;
use super::*;
use question::{Answer, Input};

mod planning;

struct Fixture {
    _directory: tempfile::TempDir,
    path: std::path::PathBuf,
    database: Database,
    events: broadcast::Sender<EventEnvelope>,
    turn: TurnId,
    session: SessionId,
    request: ToolConfirmationRequest,
}

impl Fixture {
    fn new() -> Self {
        Self::with_input(
            WorkMode::Code,
            Input::Text {
                multiline: true,
                max_bytes: 1024,
            },
        )
    }
    fn with_input(mode: WorkMode, input: Input) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("node.sqlite3");
        let mut database = Database::open(&path, NodeId([32; 32]), None).unwrap();
        let (events, _) = broadcast::channel(64);
        let Output::Project(project) = command(
            &mut database,
            &events,
            Command::RegisterProject {
                name: "Question fixture".into(),
                path: directory.path().to_str().unwrap().into(),
            },
        ) else {
            panic!("project expected");
        };
        let Output::Session(session) = command(
            &mut database,
            &events,
            Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Effort::Low,
                    mode,
                    permission: Permission::Full,
                    credential: None,
                }),
            },
        ) else {
            panic!("session expected");
        };
        let Output::QueuedTurn(turn) = command(
            &mut database,
            &events,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: 1,
                message: "fixture".into(),
            },
        ) else {
            panic!("turn expected");
        };
        runs::claim(&mut database, &events).unwrap().unwrap();
        let request = ToolConfirmationRequest {
            tool_name: "ask_user".into(),
            function_call_id: Some("question-fixture".into()),
            args: serde_json::to_value(Spec {
                prompt: "Describe the task".into(),
                input,
            })
            .unwrap(),
        };
        let mut fixture = Self {
            _directory: directory,
            path,
            database,
            events,
            turn: turn.id,
            session: session.id,
            request,
        };
        fixture.append_call();
        fixture
    }
    fn append_call(&mut self) {
        let mut event = AdkEvent::new("fixture");
        event.author = "assistant".into();
        event.set_content(adk_core::Content {
            role: "model".into(),
            parts: vec![adk_core::Part::FunctionCall {
                id: self.request.function_call_id.clone(),
                name: self.request.tool_name.clone(),
                args: self.request.args.clone(),
                thought_signature: None,
            }],
        });
        history::append(
            &mut self.database,
            self.session,
            self.turn,
            event,
            &self.events,
        )
        .unwrap();
    }
    fn begin(&mut self) -> Question {
        begin(&mut self.database, self.turn, &self.request, &self.events).unwrap()
    }
}

#[test]
fn associates_canonical_calls() {
    let mut fixture = Fixture::new();
    // Other canonical output may be appended before this sequential tool starts.
    let mut event = AdkEvent::new("fixture");
    event.author = "assistant".into();
    event.set_content(adk_core::Content::new("model").with_text("another event"));
    history::append(
        &mut fixture.database,
        fixture.session,
        fixture.turn,
        event,
        &fixture.events,
    )
    .unwrap();
    let mut wrong = fixture.request.clone();
    wrong.args["prompt"] = serde_json::json!("another question");
    assert!(begin(&mut fixture.database, fixture.turn, &wrong, &fixture.events).is_err());
    let question = fixture.begin();
    assert_eq!(question.state, State::Pending);
    assert!(
        begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            &fixture.events
        )
        .is_err()
    );
    fixture.append_call();
    assert!(calls::locate(&fixture.database.connection, fixture.turn, &fixture.request).is_err());
}

#[test]
fn retains_unconsumed_answers() {
    let mut fixture = Fixture::new();
    let question = fixture.begin();
    let answer = Response::Answer(Answer::Text(" 中文 🙂\nnext ".into()));
    let output = command(
        &mut fixture.database,
        &fixture.events,
        Command::ResolveQuestion {
            session: fixture.session,
            question: question.id,
            response: answer.clone(),
        },
    );
    let Output::Question(answered) = output else {
        panic!("question expected");
    };
    assert_eq!(
        resolve(
            &fixture.database.connection,
            fixture.session,
            question.id,
            &answer
        )
        .unwrap(),
        answered
    );
    assert_eq!(
        resolve(
            &fixture.database.connection,
            fixture.session,
            question.id,
            &Response::Cancel
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    drop(fixture.database);
    let database = Database::open(&fixture.path, NodeId([32; 32]), None).unwrap();
    let page = history::pages::read(&database.connection, fixture.session, None, 100)
        .unwrap()
        .page;
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.questions, vec![answered]);
    assert_eq!(page.runs[0].status, Status::Interrupted);
    assert_eq!(
        resolve(&database.connection, fixture.session, question.id, &answer).unwrap(),
        page.questions[0]
    );
}

#[test]
fn validates_responses() {
    let mut fixture = Fixture::new();
    let question = fixture.begin();
    let answer = Response::Answer(Answer::Text("answer".into()));
    assert_eq!(
        resolve(
            &fixture.database.connection,
            SessionId::new(),
            question.id,
            &answer
        )
        .unwrap_err()
        .code,
        ErrorCode::WrongTarget
    );
    let invalid_answer = Response::Answer(Answer::Choices {
        selected: vec![0],
        other: None,
    });
    assert_eq!(
        resolve(
            &fixture.database.connection,
            fixture.session,
            question.id,
            &invalid_answer
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
    command(
        &mut fixture.database,
        &fixture.events,
        Command::StopTurn { turn: fixture.turn },
    );
    assert_eq!(
        resolve(
            &fixture.database.connection,
            fixture.session,
            question.id,
            &answer
        )
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    runs::finish(
        &mut fixture.database,
        fixture.turn,
        Status::Cancelled,
        None,
        &fixture.events,
    )
    .unwrap();
    assert_eq!(
        get(&fixture.database.connection, question.id)
            .unwrap()
            .state,
        State::Cancelled
    );
}

#[test]
fn restart_interrupts_waiting() {
    let mut fixture = Fixture::new();
    let question = fixture.begin();
    drop(fixture.database);
    let database = Database::open(&fixture.path, NodeId([32; 32]), None).unwrap();
    let page = history::pages::read(&database.connection, fixture.session, None, 100)
        .unwrap()
        .page;
    assert_eq!(page.questions[0].id, question.id);
    assert_eq!(page.questions[0].state, State::Interrupted);
    assert_eq!(page.entries.len(), 1);
    assert!(
        resolve(
            &database.connection,
            fixture.session,
            question.id,
            &Response::Answer(Answer::Text("late".into()))
        )
        .is_err()
    );
}
