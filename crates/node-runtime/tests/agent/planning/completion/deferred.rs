use super::*;
use sailry_link::CancellationToken;

fn admission(fixture: &Fixture, kind: &str) -> Request {
    let command = match kind {
        "queue" => Command::QueueTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Further planning input".into(),
        },
        "host" => Command::SubmitTask {
            session: fixture.session.id,
            expected_revision: Some(fixture.session.revision),
            message: "Further planning input".into(),
        },
        _ => Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Further planning input".into(),
        },
    };
    fixture.client.prepare(command)
}

#[tokio::test]
async fn new_input_closes_review_without_execution_consent() {
    for remote in [false, true] {
        for kind in ["submit", "queue", "host"] {
            let server = Server::markdown("The complete plan".into()).await;
            let mut fixture = Fixture::packages(remote, &server, &["files"]).await;
            configure(&mut fixture, WorkMode::Plan).await;
            let plan = submit(&fixture).await;
            let (page, question) = questions::pending(&fixture.client, fixture.session.id).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            let other = Client::new(if remote {
                fixture.node.local()
            } else {
                fixture
                    .controller
                    .handle()
                    .remote(fixture.node.link().address())
            });
            let request = admission(&fixture, kind);
            let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap()
            else {
                panic!("queued turn expected")
            };
            assert_eq!(turn.config.mode, WorkMode::Plan);
            assert_eq!(turn.config.permission, fixture.session.config.permission);
            assert_eq!(
                fixture.client.execute(request.clone()).await.unwrap(),
                Output::QueuedTurn(turn.clone())
            );
            assert_eq!(
                other
                    .execute(other.prepare(Command::ResolveQuestion {
                        session: fixture.session.id,
                        question: question.id,
                        response: Response::StartCoding {
                            expected_revision: fixture.session.revision,
                            message: EXECUTE.into(),
                        },
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            let page = if kind == "queue" {
                history(&other, fixture.session.id).await
            } else {
                finished(&other, fixture.session.id, turn.id).await
            };
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == plan.id)
                    .unwrap()
                    .status,
                Status::Completed
            );
            assert_eq!(
                page.questions
                    .iter()
                    .find(|owned| owned.id == question.id)
                    .unwrap()
                    .state,
                State::Cancelled
            );
            let view = progress::observe(&other, fixture.session.id).await;
            let call = view
                .calls
                .iter()
                .find(|call| {
                    call.question
                        .as_ref()
                        .is_some_and(|owned| owned.id == question.id)
                })
                .unwrap();
            let call = call.with_detail(
                other
                    .tool_detail(call.detail_reference(&page).unwrap())
                    .await
                    .unwrap(),
            );
            assert!(call.question_pending(&page).is_none());
            assert_eq!(call.result(&page).unwrap()["status"], "cancelled");
            let Output::Snapshot(snapshot) = other
                .execute(other.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            assert_eq!(
                snapshot
                    .sessions
                    .iter()
                    .find(|session| session.id == fixture.session.id)
                    .unwrap()
                    .config
                    .mode,
                WorkMode::Plan
            );
            assert_eq!(
                server.requests.lock().unwrap().len(),
                if kind == "queue" { 1 } else { 2 }
            );
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn already_admitted_input_closes_review_at_claim() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let server = Server::markdown_held("The complete plan".into(), release.clone()).await;
        let mut fixture = Fixture::packages(remote, &server, &["files"]).await;
        configure(&mut fixture, WorkMode::Plan).await;
        let plan = submit(&fixture).await;
        tokio::time::timeout(Duration::from_secs(10), async {
            while server.requests.lock().unwrap().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let Output::QueuedTurn(next) = fixture
            .client
            .execute(admission(&fixture, "submit"))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        assert_eq!(next.config.mode, WorkMode::Plan);
        release.cancel();
        let page = finished(&fixture.client, fixture.session.id, next.id).await;
        let question = page
            .questions
            .iter()
            .find(|question| question.turn == plan.id)
            .unwrap();
        assert_eq!(question.state, State::Cancelled);
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == plan.id)
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ResolveQuestion {
                    session: fixture.session.id,
                    question: question.id,
                    response: Response::StartCoding {
                        expected_revision: fixture.session.revision,
                        message: EXECUTE.into(),
                    },
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn serializes_execution_with_new_input() {
    for remote in [false, true] {
        let server = Server::markdown("The complete plan".into()).await;
        let mut fixture = Fixture::packages(remote, &server, &["files"]).await;
        configure(&mut fixture, WorkMode::Plan).await;
        submit(&fixture).await;
        let (_, question) = questions::pending(&fixture.client, fixture.session.id).await;
        let other = Client::new(if remote {
            fixture.node.local()
        } else {
            fixture
                .controller
                .handle()
                .remote(fixture.node.link().address())
        });
        let execute = other.prepare(Command::ResolveQuestion {
            session: fixture.session.id,
            question: question.id,
            response: Response::StartCoding {
                expected_revision: fixture.session.revision,
                message: EXECUTE.into(),
            },
        });
        let input = admission(&fixture, "submit");
        let (accepted, admitted) = tokio::join!(
            other.execute(execute.clone()),
            fixture.client.execute(input.clone())
        );
        let (turn, state, mode) = match (accepted, admitted) {
            (Ok(Output::PlanAccepted(accepted)), Err(error)) => {
                assert_eq!(error.code, ErrorCode::RevisionConflict);
                assert_eq!(
                    other.execute(execute).await.unwrap(),
                    Output::PlanAccepted(accepted.clone())
                );
                (accepted.turn, accepted.question.state, WorkMode::Code)
            }
            (Err(error), Ok(Output::QueuedTurn(turn))) => {
                assert_eq!(error.code, ErrorCode::Conflict);
                assert_eq!(
                    fixture.client.execute(input).await.unwrap(),
                    Output::QueuedTurn(turn.clone())
                );
                (turn, State::Cancelled, WorkMode::Plan)
            }
            _ => panic!("one explicit admission must win"),
        };
        assert_eq!(turn.config.mode, mode);
        assert_eq!(turn.config.permission, fixture.session.config.permission);
        let page = finished(&other, fixture.session.id, turn.id).await;
        assert_eq!(page.runs.len(), 2);
        assert_eq!(
            page.questions
                .iter()
                .find(|owned| owned.id == question.id)
                .unwrap()
                .state,
            state
        );
        assert_eq!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part,Part::ToolResult{name,..} if name == "plan_review"))
                .count(),
            1
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
