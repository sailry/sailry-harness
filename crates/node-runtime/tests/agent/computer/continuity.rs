use super::*;

const READ: &str = "get_window_state";

fn arguments() -> Value {
    json!({"pid":314,"window_id":271,"include_screenshot":false,"query":"Draft 中文","max_depth":7})
}

fn observation() -> Value {
    driver::result(json!({"snapshot_id":"fixture-snapshot","elements":[]}))
}

async fn session(client: &Client, id: SessionId) -> Session {
    let Output::Session(session) = client
        .execute(client.prepare(Command::ReadSession { session: id }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}

async fn submit(client: &Client, id: SessionId) -> TurnId {
    let current = session(client, id).await;
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::SubmitTurn {
            session: id,
            expected_revision: current.revision,
            message: "Continue native computer tools".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn.id
}

fn binds(worker: &driver::Worker) -> Vec<Value> {
    worker
        .records()
        .into_iter()
        .filter(|record| record["operation"] == "bind_session")
        .collect()
}

fn releases(worker: &driver::Worker) -> Vec<Value> {
    worker
        .records()
        .into_iter()
        .filter(|record| {
            record["operation"] == "close_session"
                || (record["operation"] == "call" && record["name"] == "end_session")
        })
        .collect()
}

fn same_binding(worker: &driver::Worker, count: usize) {
    let bindings = binds(worker);
    assert_eq!(
        bindings.len(),
        1,
        "ordinary turns must not rebind authority"
    );
    let calls = worker.calls();
    assert_eq!(calls.len(), count, "native calls must not be replayed");
    assert!(calls[0]["session_handle"].is_string());
    for call in &calls {
        assert_eq!(call["name"], READ);
        assert_eq!(call["arguments"], arguments());
        assert_eq!(call["session_handle"], calls[0]["session_handle"]);
        assert_eq!(call["generation"], bindings[0]["generation"]);
    }
    let invocations: Vec<_> = worker
        .records()
        .into_iter()
        .filter(|record| record["operation"] == "invoke" && record["tool"] == READ)
        .collect();
    assert_eq!(invocations.len(), count);
    let mut bound_arguments = arguments();
    bound_arguments["session"] = bindings[0]["arguments"]["public_session"].clone();
    assert!(bound_arguments["session"].is_string());
    for invocation in invocations {
        assert_eq!(invocation["arguments"], bound_arguments);
        assert_eq!(invocation["session_handle"], calls[0]["session_handle"]);
    }
    assert!(
        releases(worker).is_empty(),
        "turn completion must not end or close the trusted session"
    );
}

#[tokio::test]
async fn reuses_completed_binding_after_disconnect() {
    for remote in [false, true] {
        let fixture = Fixture::with_server(
            remote,
            Server::turn_tools(vec![(READ.into(), arguments())]).await,
            WorkMode::Code,
            Permission::Full,
            true,
            json!({"get_window_state":observation()}),
        )
        .await;
        let page = finished(&fixture.client, fixture.session, fixture.turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(
            tool_result(&fixture, page.revision, READ).await,
            observation()
        );
        same_binding(&fixture.worker, 1);

        fixture
            .controller
            .handle()
            .disconnect(fixture.node.id())
            .await;
        let turn = submit(&fixture.client, fixture.session).await;
        let page = finished(&fixture.client, fixture.session, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        same_binding(&fixture.worker, 2);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 4);
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn retains_binding_after_cancelled_turn() {
    for remote in [false, true] {
        let fixture = Fixture::with_server(
            remote,
            Server::turn_tools(vec![
                (READ.into(), arguments()),
                (
                    "ask_user".into(),
                    json!({"prompt":"Continue the task","input":{"kind":"text","multiline":false,"max_bytes":1024}}),
                ),
            ])
            .await,
            WorkMode::Code,
            Permission::Full,
            true,
            json!({"get_window_state":observation()}),
        )
        .await;
        let (_, question) =
            super::super::questions::pending(&fixture.client, fixture.session).await;
        assert_eq!(question.turn, fixture.turn);
        same_binding(&fixture.worker, 1);
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StopTurn { turn: fixture.turn }),
            )
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session, fixture.turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
        same_binding(&fixture.worker, 1);

        let turn = submit(&fixture.client, fixture.session).await;
        let (_, question) =
            super::super::questions::pending(&fixture.client, fixture.session).await;
        assert_eq!(question.turn, turn);
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveQuestion {
                session: fixture.session,
                question: question.id,
                response: question::Response::Cancel,
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        same_binding(&fixture.worker, 2);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 5);
        fixture.shutdown().await;
    }
}

async fn endpoint(client: &Client, id: SessionId, endpoint: &str) {
    let current = session(client, id).await;
    let Output::Providers(providers) = client
        .execute(client.prepare(Command::ListProviders))
        .await
        .unwrap()
    else {
        panic!("providers expected")
    };
    let mut provider = providers
        .into_iter()
        .find(|provider| provider.id == current.config.provider)
        .unwrap();
    provider.endpoint = endpoint.into();
    client
        .execute(client.prepare(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        }))
        .await
        .unwrap();
}

#[tokio::test]
async fn retains_binding_after_failed_turn() {
    for remote in [false, true] {
        let fixture = Fixture::with_server(
            remote,
            Server::turn_tools(vec![(READ.into(), arguments())]).await,
            WorkMode::Code,
            Permission::Full,
            true,
            json!({"get_window_state":observation()}),
        )
        .await;
        let page = finished(&fixture.client, fixture.session, fixture.turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        same_binding(&fixture.worker, 1);

        let unavailable = Server::http(usize::MAX, 400).await;
        endpoint(&fixture.client, fixture.session, &unavailable.endpoint).await;
        let failed = submit(&fixture.client, fixture.session).await;
        let page = finished(&fixture.client, fixture.session, failed).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Failed);
        same_binding(&fixture.worker, 1);
        assert_eq!(unavailable.requests.lock().unwrap().len(), 1);

        endpoint(&fixture.client, fixture.session, &fixture.server.endpoint).await;
        let resumed = submit(&fixture.client, fixture.session).await;
        let page = finished(&fixture.client, fixture.session, resumed).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        same_binding(&fixture.worker, 2);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 4);
        fixture.shutdown().await;
    }
}

fn dispatches(worker: &driver::Worker) -> Vec<Value> {
    worker
        .records()
        .into_iter()
        .filter(|record| record["operation"] == "call")
        .collect()
}

#[tokio::test]
async fn rebinds_after_successful_end() {
    let result = driver::result(
        json!({"session":"sdk-ended-session","active":false,"unknown_extension":{"revision":7}}),
    );
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            vec![
                (READ.into(), arguments()),
                ("end_session".into(), json!({})),
            ],
            WorkMode::Code,
            Permission::Full,
            true,
            json!({"get_window_state":observation(),"end_session":result}),
        )
        .await;
        let page = finished(&fixture.client, fixture.session, fixture.turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(
            tool_result(&fixture, page.revision, "end_session").await,
            result
        );
        let original = binds(&fixture.worker);
        assert_eq!(original.len(), 1);
        let calls = dispatches(&fixture.worker);
        assert_eq!(
            calls.len(),
            2,
            "a successful end must not dispatch another end"
        );
        assert_eq!(calls[0]["name"], READ);
        assert_eq!(calls[0]["arguments"], arguments());
        assert_eq!(calls[1]["name"], "end_session");
        assert_eq!(calls[1]["arguments"], json!({}));
        let old_handle = calls[0]["session_handle"].clone();
        assert_eq!(calls[1]["session_handle"], old_handle);
        let released = releases(&fixture.worker);
        assert_eq!(released.len(), 2);
        assert_eq!(released[0]["operation"], "call");
        assert_eq!(released[1]["operation"], "close_session");
        assert!(
            released
                .iter()
                .all(|record| record["session_handle"] == old_handle)
        );

        let reader = Server::turn_tools(vec![(READ.into(), arguments())]).await;
        endpoint(&fixture.client, fixture.session, &reader.endpoint).await;
        let turn = submit(&fixture.client, fixture.session).await;
        let page = finished(&fixture.client, fixture.session, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let bindings = binds(&fixture.worker);
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0], original[0]);
        assert_ne!(
            bindings[0]["arguments"]["public_session"],
            bindings[1]["arguments"]["public_session"]
        );
        let calls = dispatches(&fixture.worker);
        assert_eq!(
            calls.len(),
            3,
            "an ended native action must not be replayed"
        );
        assert_eq!(calls[2]["name"], READ);
        assert_eq!(calls[2]["arguments"], arguments());
        assert_ne!(calls[2]["session_handle"], old_handle);
        assert_eq!(releases(&fixture.worker), released);
        let records = fixture.worker.records();
        let invoked = records
            .iter()
            .rev()
            .find(|record| record["operation"] == "invoke" && record["tool"] == READ)
            .unwrap();
        assert_eq!(
            invoked["arguments"]["session"],
            bindings[1]["arguments"]["public_session"]
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        assert_eq!(reader.requests.lock().unwrap().len(), 2);

        drop(fixture.client);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
        let released = releases(&fixture.worker);
        assert_eq!(released.len(), 4);
        assert_eq!(
            released
                .iter()
                .filter(|record| record["session_handle"] == old_handle
                    && record["operation"] == "call")
                .count(),
            1
        );
        assert_eq!(
            released
                .iter()
                .filter(|record| record["session_handle"] == old_handle
                    && record["operation"] == "close_session")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn retains_binding_after_refused_end() {
    let message = "Native session end was refused";
    let result = json!({"content":[{"type":"text","text":message}],"isError":true,
        "structuredContent":{"status":"refused","refusal":{"code":"permission_denied","message":message}}});
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            vec![
                (READ.into(), arguments()),
                ("end_session".into(), json!({})),
            ],
            WorkMode::Code,
            Permission::Full,
            true,
            json!({"get_window_state":observation(),"end_session":result}),
        )
        .await;
        let page = finished(&fixture.client, fixture.session, fixture.turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(
            tool_result(&fixture, page.revision, "end_session").await,
            result
        );
        let original = binds(&fixture.worker);
        assert_eq!(original.len(), 1);
        let calls = dispatches(&fixture.worker);
        assert_eq!(calls.len(), 2, "a refusal must not dispatch fallback tools");
        assert_eq!(calls[0]["name"], READ);
        assert_eq!(calls[0]["arguments"], arguments());
        assert_eq!(calls[1]["name"], "end_session");
        assert_eq!(calls[1]["arguments"], json!({}));
        let handle = calls[0]["session_handle"].clone();
        assert_eq!(calls[1]["session_handle"], handle);
        let attempted = releases(&fixture.worker);
        assert_eq!(attempted.len(), 1);
        assert_eq!(attempted[0]["operation"], "call");

        let reader = Server::turn_tools(vec![(READ.into(), arguments())]).await;
        endpoint(&fixture.client, fixture.session, &reader.endpoint).await;
        let turn = submit(&fixture.client, fixture.session).await;
        let page = finished(&fixture.client, fixture.session, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(binds(&fixture.worker), original);
        let calls = dispatches(&fixture.worker);
        assert_eq!(calls.len(), 3, "a refused end must not be replayed");
        assert_eq!(calls[2]["name"], READ);
        assert_eq!(calls[2]["arguments"], arguments());
        assert_eq!(calls[2]["session_handle"], handle);
        assert_eq!(releases(&fixture.worker), attempted);
        let records = fixture.worker.records();
        let invoked = records
            .iter()
            .rev()
            .find(|record| record["operation"] == "invoke" && record["tool"] == READ)
            .unwrap();
        assert_eq!(
            invoked["arguments"]["session"],
            original[0]["arguments"]["public_session"]
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        assert_eq!(reader.requests.lock().unwrap().len(), 2);
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn evicts_terminal_context_without_retry() {
    for (code, message) in [
        ("authorization_revoked", "Authorization context was revoked"),
        (
            "permission_denied",
            "Permission denied: authorization context expired",
        ),
    ] {
        let result = json!({"content":[{"type":"text","text":message}],"isError":true,
            "structuredContent":{"status":"refused","refusal":{"code":code,"message":message}}});
        for remote in [false, true] {
            let mut fixture = Fixture::with_server(
                remote,
                Server::turn_tools(vec![(READ.into(), arguments())]).await,
                WorkMode::Code,
                Permission::Full,
                true,
                json!({"get_window_state":result}),
            )
            .await;
            for count in 1..=2 {
                if count == 2 {
                    fixture.turn = submit(&fixture.client, fixture.session).await;
                }
                let page = finished(&fixture.client, fixture.session, fixture.turn).await;
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                assert_eq!(tool_result(&fixture, page.revision, READ).await, result);
                let calls = dispatches(&fixture.worker);
                assert_eq!(
                    calls.len(),
                    count,
                    "a terminal refusal must not retry the action"
                );
                let bindings = binds(&fixture.worker);
                assert_eq!(bindings.len(), count);
                let released = releases(&fixture.worker);
                assert_eq!(released.len(), count);
                for call in &calls {
                    assert_eq!(call["name"], READ, "cleanup must not dispatch end_session");
                    assert_eq!(call["arguments"], arguments());
                    assert_eq!(
                        released
                            .iter()
                            .filter(|record| record["operation"] == "close_session"
                                && record["session_handle"] == call["session_handle"])
                            .count(),
                        1
                    );
                }
                if count == 2 {
                    assert_ne!(calls[0]["session_handle"], calls[1]["session_handle"]);
                    assert_ne!(
                        bindings[0]["arguments"]["public_session"],
                        bindings[1]["arguments"]["public_session"]
                    );
                }
            }
            assert_eq!(fixture.server.requests.lock().unwrap().len(), 4);
            let released = releases(&fixture.worker);
            drop(fixture.client);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
            assert_eq!(releases(&fixture.worker), released);
            assert_eq!(dispatches(&fixture.worker).len(), 2);
            assert_eq!(binds(&fixture.worker).len(), 2);
        }
    }
}

#[tokio::test]
async fn isolates_sessions_and_releases_only_removed_owner() {
    for remote in [false, true] {
        let fixture = Fixture::with_server(
            remote,
            Server::turn_tools(vec![(READ.into(), arguments())]).await,
            WorkMode::Code,
            Permission::Full,
            true,
            json!({"get_window_state":observation()}),
        )
        .await;
        let page = finished(&fixture.client, fixture.session, fixture.turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let original = session(&fixture.client, fixture.session).await;
        let Output::Session(other) = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateSession {
                project: original.project,
                worktree: Some(original.worktree),
                config: Some(original.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        for id in [other.id, original.id] {
            let turn = submit(&fixture.client, id).await;
            let page = finished(&fixture.client, id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        }
        let bindings = binds(&fixture.worker);
        assert_eq!(bindings.len(), 2);
        assert_ne!(
            bindings[0]["arguments"]["public_session"],
            bindings[1]["arguments"]["public_session"]
        );
        let calls = fixture.worker.calls();
        assert_eq!(calls.len(), 3);
        let original_handle = &calls[0]["session_handle"];
        let other_handle = &calls[1]["session_handle"];
        assert_ne!(original_handle, other_handle);
        assert_eq!(&calls[2]["session_handle"], original_handle);
        for call in &calls {
            assert_eq!(call["name"], READ);
            assert_eq!(call["arguments"], arguments());
            assert_eq!(call["generation"], bindings[0]["generation"]);
        }
        let public_sessions: Vec<_> = fixture
            .worker
            .records()
            .into_iter()
            .filter(|record| record["operation"] == "invoke" && record["tool"] == READ)
            .map(|record| record["arguments"]["session"].clone())
            .collect();
        assert_eq!(
            public_sessions,
            vec![
                bindings[0]["arguments"]["public_session"].clone(),
                bindings[1]["arguments"]["public_session"].clone(),
                bindings[0]["arguments"]["public_session"].clone(),
            ]
        );
        assert!(releases(&fixture.worker).is_empty());

        let current = session(&fixture.client, original.id).await;
        let remove = fixture.client.prepare(Command::RemoveSession {
            session: original.id,
            expected_revision: current.revision,
        });
        let removed = fixture.client.execute(remove.clone()).await.unwrap();
        assert_eq!(removed, Output::SessionsRemoved(vec![original.id]));
        let released = releases(&fixture.worker);
        assert_eq!(released.len(), 2);
        assert!(
            released
                .iter()
                .all(|record| &record["session_handle"] == original_handle)
        );
        assert_eq!(fixture.client.execute(remove).await.unwrap(), removed);
        assert_eq!(releases(&fixture.worker), released);

        let turn = submit(&fixture.client, other.id).await;
        let page = finished(&fixture.client, other.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let calls = fixture.worker.calls();
        assert_eq!(calls.len(), 4);
        assert_eq!(&calls[3]["session_handle"], other_handle);
        assert_eq!(calls[3]["arguments"], arguments());
        assert_eq!(binds(&fixture.worker).len(), 2);
        assert_eq!(releases(&fixture.worker), released);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 8);

        drop(fixture.client);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
        let released = releases(&fixture.worker);
        assert_eq!(released.len(), 4);
        for handle in [original_handle, other_handle] {
            assert_eq!(
                released
                    .iter()
                    .filter(|record| &record["session_handle"] == handle
                        && record["operation"] == "close_session")
                    .count(),
                1
            );
            assert_eq!(
                released
                    .iter()
                    .filter(|record| &record["session_handle"] == handle
                        && record["operation"] == "call"
                        && record["name"] == "end_session")
                    .count(),
                1
            );
        }
    }
}
