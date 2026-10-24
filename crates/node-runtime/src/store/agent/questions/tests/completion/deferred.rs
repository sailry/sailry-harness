use super::*;

async fn dispatch(fixture: &mut Fixture, request: Request) -> Result<Output, Fault> {
    let (reply, response) = oneshot::channel();
    assert!(
        fixture
            .database
            .dispatch(fixture.database.node, request, reply, &fixture.events)
            .await
            .is_none()
    );
    response.await.unwrap()?.completion.await.unwrap()
}

async fn complete(fixture: &mut Fixture) -> Question {
    let source = source();
    let id = source.id.clone();
    append(fixture, source).await.unwrap();
    runs::finish_with(
        &mut fixture.database,
        fixture.turn,
        Status::Completed,
        None,
        Some(review::event(fixture.turn, id)),
        &fixture.events,
    )
    .await
    .unwrap();
    history::pages::read(&fixture.database.connection, fixture.session, None, 100)
        .await
        .unwrap()
        .page
        .questions[0]
        .clone()
}

fn choice(fixture: &Fixture, question: &Question, execute: bool) -> Request {
    Request::new(
        fixture.database.node,
        Command::ResolveQuestion {
            session: fixture.session,
            question: question.id,
            response: if execute {
                Response::StartCoding {
                    expected_revision: 1,
                    message: "Implement the accepted plan".into(),
                }
            } else {
                Response::Answer(Answer::Text(String::new()))
            },
        },
    )
}

async fn results(fixture: &Fixture) -> usize {
    history::pages::read(&fixture.database.connection, fixture.session, None, 100)
        .await
        .unwrap()
        .page
        .entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter(|part| matches!(part,Part::ToolResult{name,..} if name == review::TOOL))
        .count()
}

#[tokio::test]
async fn preserves_completed_pending_review_after_recovery() {
    let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan).await;
    let question = complete(&mut fixture).await;
    assert_eq!(
        runs::get(&fixture.database.connection, fixture.turn)
            .await
            .unwrap()
            .status,
        Status::Completed
    );
    assert_eq!(question.state, State::Pending);
    assert!(
        review::completed(&fixture.database.connection, &question)
            .await
            .unwrap()
    );
    assert_eq!(
        activity::read(&fixture.database.connection, fixture.session)
            .await
            .unwrap()
            .waiting,
        None
    );
    {
        let transaction = fixture
            .database
            .connection
            .unchecked_transaction()
            .await
            .unwrap();
        runs::recover(&transaction).await.unwrap();
        transaction.commit().await.unwrap();
    }
    assert_eq!(
        get(&fixture.database.connection, question.id)
            .await
            .unwrap(),
        question
    );
    assert_eq!(results(&fixture).await, 0);
}

#[tokio::test]
async fn writes_explicit_choices_once() {
    for execute in [false, true] {
        let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan).await;
        let question = complete(&mut fixture).await;
        let request = choice(&fixture, &question, execute);
        let output = dispatch(&mut fixture, request.clone()).await.unwrap();
        assert_eq!(
            dispatch(&mut fixture, request.clone()).await.unwrap(),
            output
        );
        let session =
            crate::store::commands::session(&fixture.database.connection, fixture.session)
                .await
                .unwrap();
        assert_eq!(session.config.permission, Permission::Full);
        assert_eq!(
            session.config.mode,
            if execute {
                WorkMode::Code
            } else {
                WorkMode::Plan
            }
        );
        assert_eq!(session.revision, if execute { 2 } else { 1 });
        assert_eq!(results(&fixture).await, 1);
        if execute {
            let Output::PlanAccepted(accepted) = output else {
                panic!("accepted plan expected")
            };
            assert_eq!(accepted.turn.request, request.id);
            assert_eq!(accepted.turn.config, session.config);
            assert_eq!(
                accepted.question.state,
                State::Answered(Answer::Plan {
                    turn: accepted.turn.id
                })
            );
            let late = choice(&fixture, &question, true);
            assert_eq!(
                dispatch(&mut fixture, late).await.unwrap_err().code,
                ErrorCode::Conflict
            );
        } else {
            let again = choice(&fixture, &question, false);
            assert!(matches!(
                dispatch(&mut fixture, again).await.unwrap(),
                Output::Question(_)
            ));
            assert_eq!(results(&fixture).await, 1);
            assert!(
                queue::read(&fixture.database.connection, fixture.session)
                    .await
                    .unwrap()
                    .items
                    .is_empty()
            );
        }
    }
}

#[tokio::test]
async fn closes_review_at_user_admission() {
    for kind in ["submit", "queue", "host"] {
        let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan).await;
        let question = complete(&mut fixture).await;
        let command = match kind {
            "submit" => Command::SubmitTurn {
                session: fixture.session,
                expected_revision: 1,
                message: "Further planning input".into(),
            },
            "queue" => Command::QueueTurn {
                session: fixture.session,
                expected_revision: 1,
                message: "Further planning input".into(),
            },
            _ => Command::SubmitTask {
                session: fixture.session,
                expected_revision: Some(1),
                message: "Further planning input".into(),
            },
        };
        let request = Request::new(fixture.database.node, command);
        let output = dispatch(&mut fixture, request.clone()).await.unwrap();
        assert_eq!(dispatch(&mut fixture, request).await.unwrap(), output);
        let Output::QueuedTurn(turn) = output else {
            panic!("queued turn expected")
        };
        assert_eq!(turn.config.mode, WorkMode::Plan);
        assert_eq!(
            get(&fixture.database.connection, question.id)
                .await
                .unwrap()
                .state,
            State::Cancelled
        );
        assert_eq!(results(&fixture).await, 1);
        let late = choice(&fixture, &question, true);
        assert_eq!(
            dispatch(&mut fixture, late).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(
            crate::store::commands::session(&fixture.database.connection, fixture.session)
                .await
                .unwrap()
                .config
                .mode,
            WorkMode::Plan
        );
    }
}

#[tokio::test]
async fn closes_nested_submit_in_the_transaction() {
    use sailry_protocol::plugin::transaction::Operation;
    for commit in [false, true] {
        let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan).await;
        let question = complete(&mut fixture).await;
        let operation = Operation::Submit {
            session: fixture.session,
            expected_revision: 1,
            message: "Further planning input".into(),
        };
        let request = Request::new(
            fixture.database.node,
            Command::PluginTransaction {
                operations: vec![operation],
            },
        );
        fixture
            .database
            .connection
            .execute(
                "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
                params![
                    &fixture.database.node.0[..],
                    request.id.to_string(),
                    serde_json::to_vec(&request).unwrap()
                ],
            )
            .await
            .unwrap();
        let mut child = request.clone();
        child.command = Command::SubmitTurn {
            session: fixture.session,
            expected_revision: 1,
            message: "Further planning input".into(),
        };
        let transaction = fixture
            .database
            .connection
            .unchecked_transaction()
            .await
            .unwrap();
        let (output, _) = crate::store::commands::execute(
            &transaction,
            fixture.database.node,
            fixture.database.node,
            None,
            &child,
        )
        .await
        .unwrap();
        let changes = review::command(
            &transaction,
            &request,
            &Output::PluginTransaction(vec![output]),
        )
        .await
        .unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(
            get(&transaction, question.id).await.unwrap().state,
            State::Cancelled
        );
        if commit {
            transaction.commit().await.unwrap();
        } else {
            transaction.rollback().await.unwrap();
        }
        assert_eq!(
            get(&fixture.database.connection, question.id)
                .await
                .unwrap()
                .state,
            if commit {
                State::Cancelled
            } else {
                State::Pending
            }
        );
        assert_eq!(results(&fixture).await, usize::from(commit));
        assert_eq!(
            queue::read(&fixture.database.connection, fixture.session)
                .await
                .unwrap()
                .items
                .len(),
            usize::from(commit)
        );
    }
}

#[tokio::test]
async fn rolls_back_failed_new_input() {
    for kind in ["revision", "empty"] {
        let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan).await;
        let question = complete(&mut fixture).await;
        let request = Request::new(
            fixture.database.node,
            Command::SubmitTurn {
                session: fixture.session,
                expected_revision: if kind == "revision" { 0 } else { 1 },
                message: if kind == "empty" { "" } else { "Valid input" }.into(),
            },
        );
        assert_eq!(
            dispatch(&mut fixture, request).await.unwrap_err().code,
            if kind == "revision" {
                ErrorCode::RevisionConflict
            } else {
                ErrorCode::InvalidRequest
            }
        );
        assert_eq!(
            get(&fixture.database.connection, question.id)
                .await
                .unwrap(),
            question
        );
        assert_eq!(results(&fixture).await, 0);
        assert!(
            queue::read(&fixture.database.connection, fixture.session)
                .await
                .unwrap()
                .items
                .is_empty()
        );
    }
}

#[tokio::test]
async fn closes_review_when_admitted_input_starts() {
    let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan).await;
    let request = Request::new(
        fixture.database.node,
        Command::SubmitTurn {
            session: fixture.session,
            expected_revision: 1,
            message: "Already queued planning input".into(),
        },
    );
    let Output::QueuedTurn(queued) = dispatch(&mut fixture, request).await.unwrap() else {
        panic!("queued turn expected")
    };
    let question = complete(&mut fixture).await;
    assert_eq!(question.state, State::Pending);
    let invocation = runs::claim(&mut fixture.database, &fixture.events)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invocation.turn.id, queued.id);
    assert_eq!(invocation.turn.config.mode, WorkMode::Plan);
    assert_eq!(
        get(&fixture.database.connection, question.id)
            .await
            .unwrap()
            .state,
        State::Cancelled
    );
    assert_eq!(results(&fixture).await, 1);
    let late = choice(&fixture, &question, true);
    assert_eq!(
        dispatch(&mut fixture, late).await.unwrap_err().code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn excludes_unsuccessful_and_ordinary_questions() {
    for status in [Status::Cancelled, Status::Failed, Status::Interrupted] {
        let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan).await;
        let source = source();
        let id = source.id.clone();
        append(&mut fixture, source).await.unwrap();
        runs::finish_with(
            &mut fixture.database,
            fixture.turn,
            status,
            None,
            Some(review::event(fixture.turn, id)),
            &fixture.events,
        )
        .await
        .unwrap();
        assert!(
            history::pages::read(&fixture.database.connection, fixture.session, None, 100)
                .await
                .unwrap()
                .page
                .questions
                .is_empty()
        );
    }
    let mut fixture = Fixture::with_input(WorkMode::Plan, Input::Plan).await;
    let mut question = fixture.begin().await;
    runs::finish(
        &mut fixture.database,
        fixture.turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .await
    .unwrap();
    question.state = State::Pending;
    put(&fixture.database.connection, &question).await.unwrap();
    assert!(
        !review::completed(&fixture.database.connection, &question)
            .await
            .unwrap()
    );
    let request = choice(&fixture, &question, true);
    assert_eq!(
        dispatch(&mut fixture, request).await.unwrap_err().code,
        ErrorCode::Conflict
    );
}
