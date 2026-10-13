use super::*;
use question::{Answer, Response, State};

const EXECUTE: &str = "Implement the accepted plan";

#[path = "completion/deferred.rs"]
mod deferred;

async fn submit(fixture: &Fixture) -> QueuedTurn {
    let Output::QueuedTurn(turn) = fixture
        .client
        .execute(fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Inspect the project and propose a plan".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn
}

#[tokio::test]
async fn completes_before_choice_and_recovers() {
    for remote in [false, true] {
        for execute in [false, true] {
            let server = Server::prompt_tools(EXECUTE.into(), vec![(
                plugin_tool("files", "write_file"),
                json!({"path":"planned.txt","text":"Reviewed output 中文 🙂","expected_revision":null}),
            )]).await;
            let mut fixture = Fixture::new(remote, &server).await;
            if !execute {
                let Output::Providers(mut providers) = fixture
                    .client
                    .execute(fixture.client.prepare(Command::ListProviders))
                    .await
                    .unwrap()
                else {
                    panic!("providers expected")
                };
                let mut provider = providers.remove(0);
                for model in &mut provider.models {
                    model
                        .capabilities
                        .get_or_insert_with(Default::default)
                        .tools = Some(false);
                }
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::PutProvider {
                        expected_revision: provider.revision,
                        provider,
                    }))
                    .await
                    .unwrap();
            }
            configure(&mut fixture, WorkMode::Plan).await;
            let plan = submit(&fixture).await;
            let session = fixture.session.id;
            let (page, question) = questions::pending(&fixture.client, session).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            assert_eq!(page.questions.len(), 1);
            assert_eq!(question.turn, plan.id);
            assert!(page.approvals.is_empty());
            assert!(!fixture.root.join("planned.txt").exists());
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            if !execute {
                assert!(
                    !server.requests.lock().unwrap()[0]["tools"]
                        .to_string()
                        .contains("ask_user")
                );
            }
            let view = progress::observe(&fixture.client, session).await;
            assert_eq!(view.calls.len(), 1);
            assert_eq!(view.calls[0].name, "plan_review");
            assert_eq!(view.calls[0].question.as_ref(), Some(&question));
            assert_eq!(
                view.calls[0].question_spec(&page).unwrap(),
                question::Spec {
                    prompt: String::new(),
                    input: question::Input::Plan,
                }
            );
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| entry.author == "assistant")
                    .flat_map(|entry| &entry.parts)
                    .filter_map(|part| match part {
                        Part::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
                "answer-fixture-a"
            );

            fixture
                .controller
                .handle()
                .disconnect(fixture.node.id())
                .await;
            let other = Client::new(if remote {
                fixture.node.local()
            } else {
                fixture
                    .controller
                    .handle()
                    .remote(fixture.node.link().address())
            });
            assert_eq!(progress::observe(&other, session).await.calls, view.calls);
            let request = other.prepare(Command::ResolveQuestion {
                session,
                question: question.id,
                response: if execute {
                    Response::StartCoding {
                        expected_revision: fixture.session.revision,
                        message: EXECUTE.into(),
                    }
                } else {
                    Response::Answer(Answer::Text(String::new()))
                },
            });
            if execute {
                let stale = other.prepare(Command::ResolveQuestion {
                    session,
                    question: question.id,
                    response: Response::StartCoding {
                        expected_revision: fixture.session.revision - 1,
                        message: EXECUTE.into(),
                    },
                });
                assert_eq!(
                    other.execute(stale).await.unwrap_err().code,
                    ErrorCode::RevisionConflict
                );
                assert_eq!(
                    history(&other, session).await.questions,
                    vec![question.clone()]
                );
                assert_eq!(server.requests.lock().unwrap().len(), 1);
            }
            let admission = other.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            drop(admission);
            let output = other.execute(request.clone()).await.unwrap();
            let page = if execute {
                let Output::PlanAccepted(accepted) = &output else {
                    panic!("accepted plan expected")
                };
                assert_eq!(accepted.turn.request, request.id);
                assert_eq!(accepted.session.config.mode, WorkMode::Code);
                assert_eq!(
                    accepted.turn.config.permission,
                    fixture.session.config.permission
                );
                assert_eq!(accepted.turn.config, accepted.session.config);
                assert_eq!(
                    accepted.question.state,
                    State::Answered(Answer::Plan {
                        turn: accepted.turn.id
                    })
                );
                let page = finished(&other, session, accepted.turn.id).await;
                assert_eq!(page.runs.len(), 2);
                assert!(page.runs.iter().all(|run| run.status == Status::Completed));
                assert_eq!(
                    std::fs::read_to_string(fixture.root.join("planned.txt")).unwrap(),
                    "Reviewed output 中文 🙂"
                );
                assert_eq!(page.approvals.len(), 1);
                assert_eq!(page.approvals[0].turn, accepted.turn.id);
                assert_eq!(server.requests.lock().unwrap().len(), 3);
                page
            } else {
                let page = finished(&other, session, plan.id).await;
                assert_eq!(page.runs.len(), 1);
                assert_eq!(page.runs[0].status, Status::Completed);
                assert_eq!(
                    page.questions[0].state,
                    State::Answered(Answer::Text(String::new()))
                );
                assert_eq!(server.requests.lock().unwrap().len(), 1);
                assert!(!fixture.root.join("planned.txt").exists());
                let Output::Snapshot(snapshot) = other
                    .execute(other.prepare(Command::Snapshot))
                    .await
                    .unwrap()
                else {
                    panic!("snapshot expected")
                };
                assert_eq!(snapshot.sessions[0].config.mode, WorkMode::Plan);
                page
            };
            assert_eq!(page.questions.len(), 1);
            assert_eq!(other.execute(request.clone()).await.unwrap(), output);
            let count = server.requests.lock().unwrap().len();
            for model_request in server.requests.lock().unwrap().iter() {
                assert!(
                    !model_request["messages"]
                        .to_string()
                        .contains("plan_review")
                );
                assert!(!model_request["tools"].to_string().contains("plan_review"));
            }
            fixture.node.shutdown().await.unwrap();
            let node = Node::start(fixture.directory.path().join("node"))
                .await
                .unwrap();
            let resumed = Client::new(if remote {
                node.local()
            } else {
                fixture.controller.handle().remote(node.link().address())
            });
            assert_eq!(history(&resumed, session).await, page);
            assert_eq!(resumed.execute(request).await.unwrap(), output);
            assert_eq!(server.requests.lock().unwrap().len(), count);
            node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn keeps_review_across_stop_and_restart() {
    let content = "## Plan\n\nPreserve this complete plan 中文 🙂\n".repeat(800);
    assert!(content.len() > 16 * 1024);
    for remote in [false, true] {
        for stop in [false, true] {
            let server = Server::markdown(content.clone()).await;
            let mut fixture = Fixture::new(remote, &server).await;
            configure(&mut fixture, WorkMode::Plan).await;
            let plan = submit(&fixture).await;
            let session = fixture.session.id;
            let (page, question) = questions::pending(&fixture.client, session).await;
            assert!(
                page.entries
                    .iter()
                    .any(|entry| entry.parts.contains(&Part::Text(content.clone())))
            );
            if stop {
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::StopTurn { turn: plan.id }))
                    .await
                    .unwrap();
                finished(&fixture.client, session, plan.id).await;
            }
            fixture.node.shutdown().await.unwrap();
            let node = Node::start(fixture.directory.path().join("node"))
                .await
                .unwrap();
            let resumed = Client::new(if remote {
                fixture.controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            let page = history(&resumed, session).await;
            assert_eq!(page.runs.len(), 1);
            assert_eq!(page.runs[0].status, Status::Completed);
            assert_eq!(page.questions.len(), 1);
            assert_eq!(page.questions[0], question);
            assert_eq!(page.questions[0].state, State::Pending);
            let Output::Snapshot(snapshot) = resumed
                .execute(resumed.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            assert!(snapshot.sessions[0].activity.waiting.is_none());
            resumed
                .execute(resumed.prepare(Command::ResolveQuestion {
                    session,
                    question: question.id,
                    response: Response::Answer(Answer::Text(String::new())),
                }))
                .await
                .unwrap();
            let resolved = history(&resumed, session).await;
            assert_eq!(
                resolved.questions[0].state,
                State::Answered(Answer::Text(String::new()))
            );
            assert!(resolved.entries.iter().any(|entry| entry.parts.iter().any(|part| {
                matches!(part,Part::ToolResult{name,result,..} if name == "plan_review" && result["status"] == "answered")
            })));
            assert!(
                page.entries
                    .iter()
                    .any(|entry| entry.parts.contains(&Part::Text(content.clone())))
            );
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn leaves_code_completion_unchanged() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(page.questions.is_empty());
        assert!(
            page.entries
                .iter()
                .all(|entry| entry.author != "plan_review")
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
