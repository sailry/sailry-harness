use super::*;
use question::{Answer, Response, State};

async fn fixture(remote: bool, model: &Server, mode: &str) -> process::Fixture {
    let mut fixture = process::Fixture::new(remote, model).await;
    package(
        &fixture.root,
        json!({"native":peer::config("mcp::peer::stdio_peer", mode)}),
        "input",
    );
    install(&fixture, 0).await;

    let mut config = fixture.session.config.clone();
    config.permission = Permission::Full;
    let Output::Session(session) = execute(
        &fixture.client,
        Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    fixture.session = session;
    fixture
}

fn values() -> Value {
    json!({"title":"Report 中文🙂","copies":2,"ratio":1.5,"publish":false,"format":"md","sections":["summary","details"]})
}

#[tokio::test]
async fn preserves_resumed_batches() {
    for remote in [false, true] {
        let model = Server::tools(vec![(alias("native", "read"), json!({})); 3]).await;
        let fixture = fixture(remote, &model, "elicitation").await;
        let (request, turn) = submit(&fixture, false).await;
        let (page, first) = questions::pending(&fixture.client, fixture.session.id).await;
        assert!(page.approvals.is_empty());
        let entry = page
            .entries
            .iter()
            .find(|entry| entry.id == first.entry)
            .unwrap();
        let Part::ToolCall {
            name, arguments, ..
        } = &entry.parts[first.index]
        else {
            panic!("input call expected")
        };
        assert_eq!(name, "mcp_elicitation");
        let spec: question::Spec = serde_json::from_value(arguments.clone()).unwrap();
        let question::Input::Form { fields } = spec.input else {
            panic!("form expected")
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.clone())
                .collect::<Vec<_>>(),
            peer::form_schema()["properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        );
        assert_eq!(model.requests.lock().unwrap().len(), 1);
        let resolve = |response| Command::ResolveQuestion {
            session: fixture.session.id,
            question: first.id,
            response,
        };
        for value in [
            json!({}),
            {
                let mut value = values();
                value["copies"] = json!(2.5);
                value
            },
            {
                let mut value = values();
                value["email"] = json!("invalid email");
                value
            },
            {
                let mut value = values();
                value["sections"] = json!(["summary", "summary"]);
                value
            },
        ] {
            let response = Response::Answer(Answer::Form(value.as_object().unwrap().clone()));
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(resolve(response)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        fixture
            .controller
            .handle()
            .disconnect(fixture.node.id())
            .await;
        let answer = fixture
            .client
            .prepare(resolve(Response::Answer(Answer::Form(
                values().as_object().unwrap().clone(),
            ))));
        let receipt = fixture.client.execute(answer.clone()).await.unwrap();
        let (_, second) = questions::pending(&fixture.client, fixture.session.id).await;
        assert_ne!(first.id, second.id);
        execute(
            &fixture.client,
            Command::ResolveQuestion {
                session: fixture.session.id,
                question: second.id,
                response: Response::Decline,
            },
        )
        .await;
        let (_, third) = questions::pending(&fixture.client, fixture.session.id).await;
        execute(
            &fixture.client,
            Command::ResolveQuestion {
                session: fixture.session.id,
                question: third.id,
                response: Response::Cancel,
            },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(page.questions.len(), 3);
        assert_eq!(page.questions[1].state, State::Declined);
        assert_eq!(page.questions[2].state, State::Cancelled);
        let output: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { name, result, .. } if name == &alias("native", "read") => {
                    Some(result)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            output[0]["output"],
            json!({"action":"accept","content":values()})
        );
        assert_eq!(output[1]["output"]["action"], "decline");
        assert_eq!(output[2]["output"]["action"], "cancel");
        let requests = model.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        assert!(
            !serde_json::to_string(&requests)
                .unwrap()
                .contains("mcp_elicitation")
        );
        assert!(
            !serde_json::to_string(&requests)
                .unwrap()
                .contains("Do not submit this automatically")
        );
        let data = fixture.node.profile().join("plugins/data/example");
        let process::Fixture {
            directory,
            node,
            controller,
            client,
            session,
            ..
        } = fixture;
        drop(client);
        node.shutdown().await.unwrap();
        reaped(&data);
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, session.id).await, page);
        assert_eq!(client.execute(answer).await.unwrap(), receipt);
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(fs::read_to_string(data.join("launches")).unwrap(), "x");
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn closes_cancelled_input() {
    for remote in [false, true] {
        for server_cancel in [false, true] {
            let model = Server::tools(vec![(alias("native", "read"), json!({}))]).await;
            let fixture = fixture(
                remote,
                &model,
                if server_cancel {
                    "elicitation_cancel"
                } else {
                    "elicitation"
                },
            )
            .await;
            let (_, turn) = submit(&fixture, false).await;
            let (_, question) = questions::pending(&fixture.client, fixture.session.id).await;
            if server_cancel {
                fs::write(
                    fixture
                        .node
                        .profile()
                        .join("plugins/data/example/cancel_input"),
                    "cancel",
                )
                .unwrap();
                tokio::time::timeout(Duration::from_secs(10), async {
                    loop {
                        if history(&fixture.client, fixture.session.id).await.questions[0].state
                            == State::Cancelled
                        {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
            }
            execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert!(matches!(
                page.questions[0].state,
                State::Cancelled | State::Interrupted
            ));
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ResolveQuestion {
                        session: fixture.session.id,
                        question: question.id,
                        response: Response::Answer(Answer::Form(
                            values().as_object().unwrap().clone()
                        ))
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            reaped(&fixture.node.profile().join("plugins/data/example"));
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
