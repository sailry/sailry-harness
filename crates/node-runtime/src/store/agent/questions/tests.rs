use super::super::tests::command;
use super::*;
use crate::agent::canonical::{BlockKind, Role as MessageRole};
use crate::store::agent::tests::fixture::{message, request as tool_request};
use question::{Answer, Input};

mod completion;
mod expiry;
mod planning;

struct Fixture {
    path: std::path::PathBuf,
    database: Database,
    events: broadcast::Sender<EventEnvelope>,
    turn: TurnId,
    session: SessionId,
    request: ToolIdentity,
    _directory: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_input(
            WorkMode::Code,
            Input::Text {
                multiline: true,
                max_bytes: 1024,
            },
        )
        .await
    }
    async fn with_input(mode: WorkMode, input: Input) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().canonicalize().unwrap().join("node.turso");
        let mut database = Database::open(&path, NodeId([32; 32]), None).await.unwrap();
        let (events, _) = broadcast::channel(64);
        let Output::Project(project) = command(
            &mut database,
            &events,
            Command::RegisterProject {
                name: "Question fixture".into(),
                path: directory.path().to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected");
        };
        let Output::Session(session) = command(
            &mut database,
            &events,
            Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    features: Default::default(),
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Effort::Low,
                    mode,
                    options: Default::default(),
                    permission: Permission::Full,
                    credential: None,
                }),
            },
        )
        .await
        else {
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
        )
        .await
        else {
            panic!("turn expected");
        };
        runs::claim(&mut database, &events).await.unwrap().unwrap();
        let mut event = message(
            "user",
            MessageRole::User,
            vec![BlockKind::Text {
                text: "fixture".into(),
            }],
        );
        event.turn = turn.id;
        event.metadata.input = Some(crate::agent::canonical::Input {
            turn: Some(turn.id),
            source: Some(crate::agent::canonical::InputSource::User),
            text_part: Some(0),
            ..Default::default()
        });
        history::append(&mut database, session.id, turn.id, event, &events)
            .await
            .unwrap();
        let request = ToolIdentity {
            name: "ask_user".into(),
            id: Some("question-fixture".into()),
            arguments: serde_json::to_value(Spec {
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
        fixture.append_call().await;
        fixture
    }
    async fn append_call(&mut self) {
        let mut event = message(
            "assistant",
            MessageRole::Assistant,
            vec![BlockKind::ToolRequest(tool_request(
                self.request.id.as_deref().expect("call identity"),
                &self.request.name,
                self.request.arguments.clone(),
            ))],
        );
        event.turn = self.turn;
        history::append(
            &mut self.database,
            self.session,
            self.turn,
            event,
            &self.events,
        )
        .await
        .unwrap();
    }
    async fn begin(&mut self) -> Question {
        begin(&mut self.database, self.turn, &self.request, &self.events)
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn associates_canonical_calls() {
    let mut fixture = Fixture::new().await;
    // Other canonical output may be appended before this sequential tool starts.
    let mut event = message(
        "assistant",
        MessageRole::Assistant,
        vec![BlockKind::Text {
            text: "another event".into(),
        }],
    );
    event.turn = fixture.turn;
    history::append(
        &mut fixture.database,
        fixture.session,
        fixture.turn,
        event,
        &fixture.events,
    )
    .await
    .unwrap();
    let mut wrong = fixture.request.clone();
    wrong.arguments["prompt"] = serde_json::json!("another question");
    assert!(
        begin(&mut fixture.database, fixture.turn, &wrong, &fixture.events)
            .await
            .is_err()
    );
    let question = fixture.begin().await;
    assert_eq!(question.state, State::Pending);
    assert!(
        begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            &fixture.events
        )
        .await
        .is_err()
    );
    fixture.append_call().await;
    assert!(
        calls::locate(&fixture.database.connection, fixture.turn, &fixture.request)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn retains_unconsumed_answers() {
    let mut fixture = Fixture::new().await;
    let question = fixture.begin().await;
    let answer = Response::Answer(Answer::Text(" 中文 🙂\nnext ".into()));
    let output = command(
        &mut fixture.database,
        &fixture.events,
        Command::ResolveQuestion {
            session: fixture.session,
            question: question.id,
            response: answer.clone(),
        },
    )
    .await;
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
        .await
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
        .await
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    fixture.database.close().await.unwrap();
    let database = Database::open(&fixture.path, NodeId([32; 32]), None)
        .await
        .unwrap();
    let page = history::pages::read(&database.connection, fixture.session, None, 100)
        .await
        .unwrap()
        .page;
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries[0].author, "user");
    assert_eq!(page.entries[0].parts, vec![Part::Text("fixture".into())]);
    assert_eq!(page.entries[1].author, "assistant");
    assert_eq!(page.questions, vec![answered]);
    assert_eq!(page.runs[0].status, Status::Interrupted);
    assert_eq!(
        resolve(&database.connection, fixture.session, question.id, &answer)
            .await
            .unwrap(),
        page.questions[0]
    );
}

#[tokio::test]
async fn validates_responses() {
    let mut fixture = Fixture::new().await;
    let question = fixture.begin().await;
    let answer = Response::Answer(Answer::Text("answer".into()));
    assert_eq!(
        resolve(
            &fixture.database.connection,
            SessionId::new(),
            question.id,
            &answer
        )
        .await
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
        .await
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
    command(
        &mut fixture.database,
        &fixture.events,
        Command::StopTurn { turn: fixture.turn },
    )
    .await;
    assert_eq!(
        resolve(
            &fixture.database.connection,
            fixture.session,
            question.id,
            &answer
        )
        .await
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
    .await
    .unwrap();
    assert_eq!(
        get(&fixture.database.connection, question.id)
            .await
            .unwrap()
            .state,
        State::Cancelled
    );
}

#[tokio::test]
async fn restart_interrupts_waiting() {
    let mut fixture = Fixture::new().await;
    let question = fixture.begin().await;
    fixture.database.close().await.unwrap();
    let database = Database::open(&fixture.path, NodeId([32; 32]), None)
        .await
        .unwrap();
    let page = history::pages::read(&database.connection, fixture.session, None, 100)
        .await
        .unwrap()
        .page;
    assert_eq!(page.questions[0].id, question.id);
    assert_eq!(page.questions[0].state, State::Interrupted);
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries[0].author, "user");
    assert_eq!(page.entries[0].parts, vec![Part::Text("fixture".into())]);
    assert_eq!(page.entries[1].author, "assistant");
    assert!(
        resolve(
            &database.connection,
            fixture.session,
            question.id,
            &Response::Answer(Answer::Text("late".into()))
        )
        .await
        .is_err()
    );
}
