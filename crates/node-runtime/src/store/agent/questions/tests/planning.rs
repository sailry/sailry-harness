use super::*;
use crate::store::commands;

fn dispatch(fixture: &mut Fixture, request: Request) -> Result<Output, Fault> {
    let (reply, response) = oneshot::channel();
    assert!(
        fixture
            .database
            .dispatch(fixture.database.node, request, reply, &fixture.events)
            .is_none()
    );
    response
        .blocking_recv()
        .unwrap()?
        .completion
        .blocking_recv()
        .unwrap()
}

fn request(fixture: &Fixture, question: QuestionId, expected_revision: u64, text: &str) -> Request {
    Request::new(
        fixture.database.node,
        Command::ResolveQuestion {
            session: fixture.session,
            question,
            response: Response::StartCoding {
                expected_revision,
                message: text.into(),
            },
        },
    )
}

#[test]
fn requires_frozen_planning_authority() {
    let mut fixture = Fixture::with_input(WorkMode::Code, Input::Plan);
    assert_eq!(
        begin(
            &mut fixture.database,
            fixture.turn,
            &fixture.request,
            &fixture.events
        )
        .unwrap_err()
        .code,
        ErrorCode::PermissionDenied
    );
    let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan);
    let mut config = commands::session(&fixture.database.connection, fixture.session)
        .unwrap()
        .config;
    config.mode = WorkMode::Code;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: fixture.session,
            expected_revision: 1,
            config,
        },
    );
    let question = fixture.begin();
    assert_eq!(question.state, State::Pending);
    assert_eq!(
        resolve(
            &fixture.database.connection,
            fixture.session,
            question.id,
            &Response::Answer(Answer::Plan {
                turn: TurnId::new()
            })
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn rolls_back_invalid_acceptance() {
    for case in ["revision", "empty", "full", "ordinary", "target"] {
        let mut fixture = Fixture::with_input(
            WorkMode::Plan,
            if case == "ordinary" {
                Input::Text {
                    multiline: true,
                    max_bytes: 1024,
                }
            } else {
                Input::Plan
            },
        );
        let question = fixture.begin();
        if case == "full" {
            for _ in 0..256 {
                command(
                    &mut fixture.database,
                    &fixture.events,
                    Command::QueueTurn {
                        session: fixture.session,
                        expected_revision: 1,
                        message: "Queued input".into(),
                    },
                );
            }
        }
        let before = commands::session(&fixture.database.connection, fixture.session).unwrap();
        let queue = super::super::super::queue::read(&fixture.database.connection, fixture.session)
            .unwrap();
        let mut request = request(
            &fixture,
            question.id,
            if case == "revision" { 0 } else { 1 },
            if case == "empty" { "" } else { "Start coding" },
        );
        if case == "target" {
            let Command::ResolveQuestion { session, .. } = &mut request.command else {
                unreachable!()
            };
            *session = SessionId::new();
        }
        let error = dispatch(&mut fixture, request.clone()).unwrap_err();
        assert_eq!(
            error.code,
            match case {
                "revision" => ErrorCode::RevisionConflict,
                "full" => ErrorCode::Busy,
                "target" => ErrorCode::WrongTarget,
                _ => ErrorCode::InvalidRequest,
            }
        );
        assert_eq!(dispatch(&mut fixture, request).unwrap_err(), error);
        assert_eq!(
            commands::session(&fixture.database.connection, fixture.session).unwrap(),
            before
        );
        assert_eq!(
            get(&fixture.database.connection, question.id).unwrap(),
            question
        );
        assert_eq!(
            super::super::super::queue::read(&fixture.database.connection, fixture.session)
                .unwrap(),
            queue
        );
    }
}

#[test]
fn preserves_queued_revisions() {
    let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan);
    let question = fixture.begin();
    let Output::QueuedTurn(held) = command(
        &mut fixture.database,
        &fixture.events,
        Command::QueueTurn {
            session: fixture.session,
            expected_revision: 1,
            message: "Other queued draft".into(),
        },
    ) else {
        panic!("turn expected")
    };
    super::super::super::queue::pause(&fixture.database.connection, fixture.session, true).unwrap();
    let request = request(&fixture, question.id, 1, "Implement this plan 中文 🙂");
    let mut events = fixture.events.subscribe();
    let output = dispatch(&mut fixture, request.clone()).unwrap();
    let Output::PlanAccepted(accepted) = &output else {
        panic!("accepted plan expected")
    };
    assert_eq!(
        accepted.question.state,
        State::Answered(Answer::Plan {
            turn: accepted.turn.id
        })
    );
    assert_eq!(accepted.session.config.mode, WorkMode::Code);
    assert_eq!(accepted.session.revision, 2);
    assert_eq!(accepted.turn.revision, 2);
    assert_eq!(accepted.turn.request, request.id);
    assert_eq!(
        events.try_recv().unwrap().event,
        Event::PlanAccepted(accepted.clone())
    );
    assert_eq!(dispatch(&mut fixture, request.clone()).unwrap(), output);
    assert!(events.try_recv().is_err());
    let mut repeated = request;
    repeated.id = RequestId::new();
    assert_eq!(
        dispatch(&mut fixture, repeated).unwrap_err().code,
        ErrorCode::Conflict
    );
    let queue =
        super::super::super::queue::read(&fixture.database.connection, fixture.session).unwrap();
    assert!(!queue.paused);
    assert_eq!(
        queue.items.iter().map(|item| item.turn).collect::<Vec<_>>(),
        [accepted.turn.id, held.id]
    );
    assert_eq!(queue.items[0].preview, "Implement this plan 中文 🙂");
    assert!(!queue.items[1].ready);
    assert_eq!(
        super::super::super::queue::message(&fixture.database.connection, accepted.turn.id)
            .unwrap()
            .message
            .text,
        "Implement this plan 中文 🙂"
    );
    assert!(
        runs::claim(&mut fixture.database, &fixture.events)
            .unwrap()
            .is_none()
    );
    let mut config = accepted.session.config.clone();
    config.mode = WorkMode::Plan;
    command(
        &mut fixture.database,
        &fixture.events,
        Command::SetSessionConfig {
            session: fixture.session,
            expected_revision: 2,
            config,
        },
    );
    runs::finish(
        &mut fixture.database,
        fixture.turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    let coding = runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    assert_eq!(coding.turn, accepted.turn);
    assert_eq!(coding.turn.config.permission, Permission::Full);
    assert_eq!(
        super::super::super::permissions::config(&fixture.database.connection, fixture.turn)
            .unwrap()
            .mode,
        WorkMode::Plan
    );
}
