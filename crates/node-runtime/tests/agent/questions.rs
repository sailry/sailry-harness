use super::*;
use question::{Answer, Question, Response, State};
use serde_json::json;

pub(super) async fn pending(client: &Client, session: SessionId) -> (Page, Question) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = history(client, session).await;
            if let Some(question) = page
                .questions
                .iter()
                .find(|question| question.state == State::Pending)
                .cloned()
            {
                return (page, question);
            }
            assert!(
                !page
                    .runs
                    .last()
                    .is_some_and(|run| matches!(run.status, Status::Failed | Status::Completed)),
                "{page:?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("question deadline")
}

fn text_input() -> serde_json::Value {
    json!({"prompt": "Describe the task", "input": {"kind": "text", "multiline": true, "max_bytes": 1024}})
}

#[cfg(unix)]
#[tokio::test]
async fn separates_answers_from_approvals() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            ("ask_user".into(), text_input()),
            (
                plugin_tool("files", "write_file"),
                json!({"path": "answer.txt", "text": "requested", "expected_revision": null}),
            ),
        ])
        .await;
        let fixture = process::Fixture::new(remote, &server).await;
        let session = fixture.session.id;
        let turn = approvals::submit(&fixture.client, session).await;
        let (_, question) = pending(&fixture.client, session).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveQuestion {
                session,
                question: question.id,
                response: Response::Answer(Answer::Text("Approved, proceed".into())),
            }))
            .await
            .unwrap();
        let (page, approval) = approvals::pending(&fixture.client, session).await;
        assert!(matches!(page.questions[0].state, State::Answered(_)));
        assert_eq!(approval.source, ApprovalSource::User);
        assert!(!fixture.root.join("answer.txt").exists());
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        {
            let requests = server.requests.lock().unwrap();
            let request = &requests[0];
            let instructions = request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|message| message["content"].as_str())
                .collect::<String>();
            assert!(instructions.contains("not permission to start already requested work"));
            assert!(instructions.contains("runtime pauses file writes"));
            let question = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["function"]["name"] == "ask_user")
                .unwrap();
            assert!(
                question["function"]["description"]
                    .as_str()
                    .unwrap()
                    .contains("Answers do not grant tool permissions")
            );
        }
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        let page = finished(&fixture.client, session, turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert!(!fixture.root.join("answer.txt").exists());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn answers_and_cancels() {
    for remote in [false, true] {
        for permission in [Permission::Ask, Permission::Project, Permission::Full] {
            let fixture = tempfile::tempdir().unwrap();
            let root = fixture.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let node = Node::start(fixture.path().join("node")).await.unwrap();
            let controller =
                Link::controller(fixture.path().join("controller"), NetworkScope::default())
                    .await
                    .unwrap();
            let address = controller
                .handle()
                .pair(node.link().invite().unwrap().ticket())
                .await
                .unwrap();
            let transport = if remote {
                controller.handle().remote(address.clone())
            } else {
                node.local()
            };
            let client = Client::new(transport.clone());
            let server = Server::parallel(vec![
                ("ask_user".into(), text_input()),
                ("ask_user".into(), json!({"prompt": "Pick one", "input": {"kind": "choice", "options": ["One", "Two"], "multiple": false, "allow_other": true}})),
                ("ask_user".into(), json!({"prompt": "Pick several", "input": {"kind": "choice", "options": ["One", "Two", "Three"], "multiple": true, "allow_other": true}})),
                ("ask_user".into(), text_input()),
            ]).await;
            let session = approvals::prepare(&client, &server, &root).await;
            let mut config = session.config.clone();
            config.permission = permission;
            client
                .execute(client.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: 1,
                    config,
                }))
                .await
                .unwrap();
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: 2,
                    message: "Ask for input".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected");
            };
            let (page, first) = pending(&client, session.id).await;
            assert!(page.approvals.is_empty());
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            assert_eq!(
                client
                    .execute(client.prepare(Command::ResolveQuestion {
                        session: SessionId::new(),
                        question: first.id,
                        response: Response::Cancel
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::WrongTarget
            );
            let mut stream = client.subscribe_conversation(session.id).await.unwrap();
            let mut projection =
                sailry_client::conversation::Projection::new(client.target(), session.id, 0);
            projection.apply(0, stream.next().await.unwrap()).unwrap();
            assert_eq!(
                projection.snapshot().unwrap().page.questions,
                vec![first.clone()]
            );
            drop(stream);
            drop(client);
            let other = Client::new(node.local());
            let request = other.prepare(Command::ResolveQuestion {
                session: session.id,
                question: first.id,
                response: Response::Answer(Answer::Text(" 中文 🙂\nnext ".into())),
            });
            let response = other.execute(request.clone()).await.unwrap();
            let (page, second) = pending(&other, session.id).await;
            assert_eq!(
                page.entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .filter(|part| matches!(part, Part::ToolResult { .. }))
                    .count(),
                1,
                "the first answer is returned before the next question"
            );
            assert_eq!(other.execute(request.clone()).await.unwrap(), response);
            assert_eq!(
                other
                    .execute(other.prepare(Command::ResolveQuestion {
                        session: session.id,
                        question: first.id,
                        response: Response::Cancel
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            let resumed = Client::new(if remote {
                controller.handle().remote(address)
            } else {
                node.local()
            });
            let mut stream = resumed.subscribe_conversation(session.id).await.unwrap();
            projection.reconnect(1).unwrap();
            projection.apply(1, stream.next().await.unwrap()).unwrap();
            let invalid = Response::Answer(Answer::Choices {
                selected: vec![0, 1],
                other: None,
            });
            assert_eq!(
                resumed
                    .execute(resumed.prepare(Command::ResolveQuestion {
                        session: session.id,
                        question: second.id,
                        response: invalid
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
            let single = resumed.prepare(Command::ResolveQuestion {
                session: session.id,
                question: second.id,
                response: Response::Answer(Answer::Choices {
                    selected: vec![],
                    other: Some("Custom 中文".into()),
                }),
            });
            // Lose the completion after durable receipt; a stable retry must not answer the next question.
            let admission = transport.dispatch(single.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            drop(admission);
            let (_, third) = pending(&other, session.id).await;
            assert_ne!(third.id, second.id);
            let Output::Question(replayed) = resumed.execute(single).await.unwrap() else {
                panic!("question expected");
            };
            assert_eq!(replayed.id, second.id);
            resumed
                .execute(resumed.prepare(Command::ResolveQuestion {
                    session: session.id,
                    question: third.id,
                    response: Response::Answer(Answer::Choices {
                        selected: vec![2, 0],
                        other: Some("Extra".into()),
                    }),
                }))
                .await
                .unwrap();
            let (_, fourth) = pending(&other, session.id).await;
            resumed
                .execute(resumed.prepare(Command::ResolveQuestion {
                    session: session.id,
                    question: fourth.id,
                    response: Response::Cancel,
                }))
                .await
                .unwrap();
            let page = finished(&other, session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed, "{page:?}");
            assert_eq!(page.questions.len(), 4);
            assert!(page.approvals.is_empty());
            let results: Vec<_> = page
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| match part {
                    Part::ToolResult { result, .. } => Some(result.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                results,
                vec![
                    json!({"status": "answered", "answer": " 中文 🙂\nnext "}),
                    json!({"status": "answered", "answers": ["Custom 中文"]}),
                    json!({"status": "answered", "answers": ["Three", "One", "Extra"]}),
                    json!({"status": "cancelled"})
                ]
            );
            assert_eq!(server.requests.lock().unwrap().len(), 2);
            tokio::time::timeout(Duration::from_secs(10), async {
                while projection.snapshot().unwrap().page.runs[0].status != Status::Completed {
                    assert_eq!(
                        projection.apply(1, stream.next().await.unwrap()).unwrap(),
                        sailry_client::Apply::Applied
                    );
                }
            })
            .await
            .expect("question projection deadline");
            assert_eq!(*projection.snapshot().unwrap().page, page);
            drop(stream);
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
            let node = Node::start(fixture.path().join("node")).await.unwrap();
            let client = Client::new(node.local());
            assert_eq!(history(&client, session.id).await, page);
            assert_eq!(client.execute(request).await.unwrap(), response);
            assert_eq!(server.requests.lock().unwrap().len(), 2);
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn stop_and_shutdown() {
    for remote in [false, true] {
        for shutdown in [false, true] {
            let fixture = tempfile::tempdir().unwrap();
            let root = fixture.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let node = Node::start(fixture.path().join("node")).await.unwrap();
            let controller =
                Link::controller(fixture.path().join("controller"), NetworkScope::default())
                    .await
                    .unwrap();
            let address = controller
                .handle()
                .pair(node.link().invite().unwrap().ticket())
                .await
                .unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(address)
            } else {
                node.local()
            });
            let server = Server::tools(vec![("ask_user".into(), text_input())]).await;
            let session = approvals::prepare(&client, &server, &root).await;
            let turn = approvals::submit(&client, session.id).await;
            let (_, question) = pending(&client, session.id).await;
            if !shutdown {
                client
                    .execute(client.prepare(Command::StopTurn { turn }))
                    .await
                    .unwrap();
                let page = finished(&client, session.id, turn).await;
                assert_eq!(page.questions[0].state, State::Cancelled);
            }
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
            let node = Node::start(fixture.path().join("node")).await.unwrap();
            let client = Client::new(node.local());
            let page = history(&client, session.id).await;
            assert_eq!(
                page.questions[0].state,
                if shutdown {
                    State::Interrupted
                } else {
                    State::Cancelled
                }
            );
            assert_eq!(
                client
                    .execute(client.prepare(Command::ResolveQuestion {
                        session: session.id,
                        question: question.id,
                        response: Response::Answer(Answer::Text("late".into()))
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            node.shutdown().await.unwrap();
        }
    }
}
