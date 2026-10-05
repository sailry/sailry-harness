use super::*;
use question::{Answer, Response, State};
use sailry_client::Apply;

fn proposal(prompt: &str) -> (String, Value) {
    (
        "ask_user".into(),
        json!({"prompt": prompt, "input": {"kind": "plan"}}),
    )
}

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
async fn starts_one_coding_turn() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            proposal("Create the file with the revised content 中文 🙂"),
            (plugin_tool("files", "write_file"), json!({"path": "planned.txt", "text": "Reviewed content 中文 🙂", "expected_revision": null})),
        ]).await;
        let mut fixture = Fixture::new(remote, &server).await;
        configure(&mut fixture, WorkMode::Plan).await;
        let plan = submit(&fixture).await;
        let session = fixture.session.id;
        let (page, question) = questions::pending(&fixture.client, session).await;
        assert!(page.approvals.is_empty());
        assert!(!fixture.root.join("planned.txt").exists());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert_eq!(question.turn, plan.id);
        assert!(!fixture.root.join("planned.txt").exists());
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
        let mut inventory = other.subscribe().await.unwrap();
        let mut projection = sailry_client::Projection::new(other.target(), 0);
        assert_eq!(
            projection
                .apply(0, inventory.next().await.unwrap())
                .unwrap(),
            Apply::Applied
        );
        let initial_cursor = projection.snapshot().unwrap().cursor;
        let mut updates = other.subscribe_conversation(session).await.unwrap();
        let mut conversation =
            sailry_client::conversation::Projection::new(other.target(), session, 0);
        conversation
            .apply(0, updates.next().await.unwrap())
            .unwrap();
        let request = other.prepare(Command::ResolveQuestion {
            session,
            question: question.id,
            response: Response::StartCoding {
                expected_revision: fixture.session.revision,
                message: "Implement the accepted plan 中文 🙂".into(),
            },
        });
        // Lose completion after durable admission; the same identifier must recover the result.
        let admission = other.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        drop(admission);
        let output = other.execute(request.clone()).await.unwrap();
        let Output::PlanAccepted(accepted) = &output else {
            panic!("accepted plan expected")
        };
        assert_eq!(
            accepted.question.state,
            State::Answered(Answer::Plan {
                turn: accepted.turn.id
            })
        );
        assert_eq!(accepted.turn.request, request.id);
        assert_eq!(accepted.session.config.mode, WorkMode::Code);
        assert_eq!(accepted.turn.config, accepted.session.config);
        assert_eq!(accepted.turn.config.permission, Permission::Full);
        let page = finished(&other, session, accepted.turn.id).await;
        assert_eq!(page.runs.len(), 2, "{page:?}");
        assert!(
            page.runs.iter().all(|run| run.status == Status::Completed),
            "{page:?}"
        );
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == plan.id)
                .unwrap()
                .revision,
            plan.revision
        );
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(page.approvals[0].source, ApprovalSource::Full);
        assert_eq!(page.approvals[0].turn, accepted.turn.id);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("planned.txt")).unwrap(),
            "Reviewed content 中文 🙂"
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let update = inventory.next().await.unwrap();
                let accepted_event = matches!(&update, Update::Event(envelope) if matches!(envelope.event, Event::PlanAccepted(_)));
                // The receiver opens before snapshotting; buffered events can already be covered.
                let covered = matches!(
                    &update,
                    Update::Event(envelope) if envelope.cursor <= initial_cursor
                );
                assert_eq!(
                    projection.apply(0, update.clone()).unwrap(),
                    if covered { Apply::Ignored } else { Apply::Applied },
                    "initial cursor: {initial_cursor}, update: {update:?}"
                );
                if accepted_event {
                    assert!(!covered, "plan acceptance must follow the initial snapshot");
                    let snapshot = projection.snapshot().unwrap();
                    assert_eq!(snapshot.sessions.iter().find(|item| item.id == session).unwrap(), &accepted.session);
                    assert_eq!(snapshot.turns.iter().find(|turn| turn.id == accepted.turn.id).unwrap(), &accepted.turn);
                    assert_eq!(projection.apply(0, update.clone()).unwrap(), Apply::Ignored);
                    projection.reconnect(1).unwrap();
                    assert_eq!(projection.apply(0, update).unwrap(), Apply::Ignored);
                    break;
                }
            }
            while conversation.snapshot().unwrap().page.runs.iter().filter(|run| run.status == Status::Completed).count() != 2 {
                assert_eq!(conversation.apply(0, updates.next().await.unwrap()).unwrap(), Apply::Applied);
            }
        }).await.expect("plan projection deadline");
        assert_eq!(*conversation.snapshot().unwrap().page, page);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(
            requests.len(),
            3,
            "accepted planning run must not invoke the model again"
        );
        for (index, request) in requests.iter().enumerate() {
            let planning = index == 0;
            assert_eq!(
                tool_names(request).contains(&plugin_tool("files", "write_file").as_str()),
                !planning
            );
            assert_eq!(
                request["messages"]
                    .to_string()
                    .contains("This is a planning turn"),
                planning
            );
            let schema = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["function"]["name"] == "ask_user")
                .unwrap();
            let kinds = &schema["function"]["parameters"]["properties"]["input"]["anyOf"];
            assert_eq!(
                kinds
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|kind| kind["properties"]["kind"]["enum"] == json!(["plan"])),
                planning
            );
        }
        assert!(
            requests[1]["messages"]
                .to_string()
                .contains(&accepted.turn.id.to_string())
        );
        drop(inventory);
        drop(updates);
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
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_feedback_and_finishes() {
    for remote in [false, true] {
        let server = Server::tools(vec![proposal("Create the requested file")]).await;
        let mut fixture = Fixture::new(remote, &server).await;
        configure(&mut fixture, WorkMode::Plan).await;
        let turn = submit(&fixture).await;
        let session = fixture.session.id;
        let (_, question) = questions::pending(&fixture.client, session).await;
        let answer = Answer::Text("Revise the content 中文 🙂".into());
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveQuestion {
                session,
                question: question.id,
                response: Response::Answer(answer.clone()),
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, session, turn.id).await;
        assert_eq!(page.runs.len(), 1);
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(page.questions[0].state, State::Answered(answer));
        assert!(page.approvals.is_empty());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_closed_plans() {
    for remote in [false, true] {
        for shutdown in [false, true] {
            let server = Server::tools(vec![proposal("Create a file")]).await;
            let mut fixture = Fixture::new(remote, &server).await;
            configure(&mut fixture, WorkMode::Plan).await;
            let turn = submit(&fixture).await;
            let (_, question) = questions::pending(&fixture.client, fixture.session.id).await;
            if !shutdown {
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::StopTurn { turn: turn.id }))
                    .await
                    .unwrap();
                finished(&fixture.client, fixture.session.id, turn.id).await;
            }
            fixture.node.shutdown().await.unwrap();
            let node = Node::start(fixture.directory.path().join("node"))
                .await
                .unwrap();
            let client = Client::new(if remote {
                fixture.controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            let page = history(&client, fixture.session.id).await;
            assert_eq!(
                page.questions[0].state,
                if shutdown {
                    State::Interrupted
                } else {
                    State::Cancelled
                }
            );
            let request = client.prepare(Command::ResolveQuestion {
                session: fixture.session.id,
                question: question.id,
                response: Response::StartCoding {
                    expected_revision: fixture.session.revision,
                    message: "Implement the plan".into(),
                },
            });
            assert_eq!(
                client.execute(request).await.unwrap_err().code,
                ErrorCode::Conflict
            );
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            assert_eq!(snapshot.sessions[0].config.mode, WorkMode::Plan);
            assert_eq!(snapshot.turns.len(), 1);
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
