use super::*;
use serde_json::Value;
use std::collections::BTreeSet;
use tokio::sync::Notify;

async fn submit(client: &Client, session: &Session, message: &str) -> TurnId {
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: message.into(),
        }))
        .await
        .unwrap()
    else {
        panic!("queued turn expected")
    };
    turn.id
}
fn completed(page: &Page, turn: TurnId) {
    let run = page.runs.iter().find(|run| run.turn == turn).unwrap();
    assert_eq!(run.status, Status::Completed, "{page:?}");
    assert!(run.error.is_none(), "{run:?}");
    assert!(page.questions.is_empty(), "{page:?}");
    assert!(
        page.entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .any(|part| { matches!(part,Part::Text(text) if text=="answer-fixture-a") }),
        "{page:?}"
    );
}

async fn rejected(client: &Client, request: &Request) {
    let RequestOutcome::Completed(output) = client.outcome(request).await.unwrap() else {
        panic!("completed storage request expected")
    };
    let error = output.unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    assert_eq!(error.details, Some(FaultDetails::PluginValueConflict));
}

#[tokio::test]
async fn reconciles_changed_entries_without_restarting_turns() {
    for remote in [false, true] {
        for after_cas in [false, true] {
            for deleted in [false, true] {
                let id = MemoryId::new();
                let gate = Arc::new(Notify::new());
                let save = |revision: Value, body: Value| {
                    (
                        plugin_tool("memory", "save_memory"),
                        json!({
                            "id":id,"expected_revision":revision,"kind":"project","title":"Verified constraints","body":body,
                        }),
                    )
                };
                let mut calls = vec![
                    (plugin_tool("memory", "search_memory"), json!({"query":id})),
                    save(json!(1), json!("Keep tests runnable")),
                ];
                if !deleted {
                    calls.push(save(json!("$memory_revision"), json!("$memory_append")));
                }
                let expected_approvals = calls.len() - 1;
                let server = Server::memory_tools(calls, gate.clone()).await;
                let fixture = fixture(remote, &server).await;
                install(&fixture).await;
                assert_eq!(fixture.session.config.permission, Permission::Ask);
                let original = support::put(
                    &fixture.client,
                    Entry {
                        summary: Summary {
                            id,
                            project: fixture.session.project,
                            title: "Verified constraints".into(),
                            kind: Kind::Project,
                            revision: 0,
                            updated_at_ms: 0,
                            archived: false,
                        },
                        body: "Keep the original project boundary".into(),
                    },
                    0,
                )
                .await
                .unwrap();
                let turn = submit(
                    &fixture.client,
                    &fixture.session,
                    "Remember that tests must stay runnable",
                )
                .await;
                server.wait_count(2).await;
                let held = if after_cas {
                    let (entered, release) = fixture.node.hold_plugin_operation_fixture();
                    gate.notify_one();
                    Some((
                        tokio::time::timeout(Duration::from_secs(5), entered)
                            .await
                            .unwrap()
                            .unwrap(),
                        release,
                    ))
                } else {
                    None
                };
                if deleted {
                    support::run::<Value>(
                        &fixture.client,
                        &support::context(&fixture.client).await,
                        json!({"action":"remove","id":id,"expected_revision":1}),
                    )
                    .await
                    .unwrap();
                } else {
                    let mut current = original.clone();
                    current.body.push_str("; preserve the concurrent fact");
                    support::put(&fixture.client, current, 1).await.unwrap();
                }
                if let Some((_, release)) = &held {
                    release.cancel();
                } else {
                    gate.notify_one();
                }
                let page = finished(&fixture.client, fixture.session.id, turn).await;
                completed(&page, turn);
                if let Some((request, _)) = &held {
                    rejected(&fixture.client, request).await;
                }
                let outputs = results(&fixture.client, &page).await;
                let changed = outputs[1].get("context").unwrap_or(outputs[1]);
                assert_eq!(changed["saved"], false);
                assert_eq!(
                    changed["status"],
                    if deleted { "deleted" } else { "changed" }
                );
                if after_cas {
                    assert_eq!(outputs[1]["isError"], true);
                    assert_eq!(
                        outputs[1]["error"]["details"],
                        json!({"kind":"plugin_value_conflict"})
                    );
                } else {
                    assert!(outputs[1].get("isError").is_none());
                }
                assert!(
                    page.approvals
                        .iter()
                        .all(|approval| approval.state == ApprovalState::Approved
                            && approval.source == ApprovalSource::Storage)
                );
                assert_eq!(
                    page.approvals.len(),
                    expected_approvals,
                    "remote={remote}, after_cas={after_cas}, deleted={deleted}",
                );
                if deleted {
                    assert_eq!(outputs.len(), 2);
                    assert_eq!(
                        support::read(&fixture.client, id).await.unwrap_err().code,
                        ErrorCode::NotFound
                    );
                    assert_eq!(changed["deleted"], json!([id]));
                } else {
                    let current = support::read(&fixture.client, id).await.unwrap();
                    assert_eq!(current.summary.revision, 3);
                    assert_eq!(
                        current.body,
                        "Keep the original project boundary; preserve the concurrent fact; keep tests runnable"
                    );
                    assert_eq!(outputs[2]["updated"][0]["revision"], 3);
                    let request = server.requests.lock().unwrap().last().unwrap().to_string();
                    assert!(request.contains("changed/latest/pending is not saved"));
                    assert!(
                        request
                            .contains("reread and reconcile automatically, preserving new facts")
                    );
                    assert!(request.contains("preserve the concurrent fact"));
                }
                fixture.node.shutdown().await.unwrap();
                fixture.controller.close().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn merges_writes_from_independent_sessions() {
    for remote in [false, true] {
        let mut servers = Vec::new();
        for index in 0..6 {
            servers.push(Server::tools(vec![(plugin_tool("memory","save_memory"),json!({
                "expected_revision":0,"kind":"project","title":format!("Constraint {index}"),"body":format!("Verified independent condition {index}"),
            }))]).await);
        }
        let fixture = fixture(remote, &servers[0]).await;
        install(&fixture).await;
        let Output::Providers(providers) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let original = providers
            .into_iter()
            .find(|value| value.id == fixture.session.config.provider)
            .unwrap();
        let mut sessions = vec![fixture.session.clone()];
        for server in servers.iter().skip(1) {
            let mut provider = original.clone();
            provider.id = ProviderId::new();
            provider.revision = 0;
            provider.endpoint = server.endpoint.clone();
            let Output::Provider(provider) = fixture
                .client
                .execute(fixture.client.prepare(Command::PutProvider {
                    provider,
                    expected_revision: 0,
                }))
                .await
                .unwrap()
            else {
                panic!("provider expected")
            };
            let mut config = fixture.session.config.clone();
            config.provider = provider.id;
            let Output::Session(session) = fixture
                .client
                .execute(fixture.client.prepare(Command::CreateSession {
                    project: fixture.session.project,
                    worktree: Some(fixture.session.worktree),
                    config: Some(config),
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            sessions.push(session);
        }
        assert_eq!(fixture.session.config.permission, Permission::Ask);
        let (entered, release) = fixture.node.hold_plugin_operation_fixture();
        let mut turns = vec![
            submit(
                &fixture.client,
                &sessions[0],
                "Save the verified project constraint",
            )
            .await,
        ];
        let prepared = tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .unwrap()
            .unwrap();
        assert!(support::list(&fixture.client).await.unwrap().is_empty());
        let held: Vec<_> = sessions
            .iter()
            .skip(1)
            .map(|_| fixture.node.hold_plugin_operation_fixture())
            .collect();
        for session in sessions.iter().skip(1) {
            turns.push(
                submit(
                    &fixture.client,
                    session,
                    "Save the verified project constraint",
                )
                .await,
            );
        }
        let mut pending = Vec::new();
        for (entered, release) in held {
            let request = tokio::time::timeout(Duration::from_secs(5), entered)
                .await
                .unwrap()
                .unwrap();
            pending.push((request, release));
        }
        assert!(support::list(&fixture.client).await.unwrap().is_empty());
        // All writers prepared the same catalog. Release them in order so each
        // stale CAS is reconciled without exhausting read-snapshot acquisition.
        for (index, (session, turn)) in sessions.iter().zip(&turns).enumerate().skip(1) {
            let position = pending
                .iter()
                .position(|(request, _)| {
                    request.plugin.as_ref().is_some_and(|context| {
                        context.session == Some(session.id) && context.turn == Some(*turn)
                    })
                })
                .expect("prepared memory request for the session");
            let (_, release) = pending.swap_remove(position);
            release.cancel();
            let page = finished(&fixture.client, session.id, *turn).await;
            completed(&page, *turn);
            let output = results(&fixture.client, &page).await;
            assert_eq!(output.len(), 1, "{page:?}");
            assert_eq!(
                output[0]["updated"].as_array().map(Vec::len),
                Some(1),
                "{output:?}"
            );
            let id: MemoryId =
                serde_json::from_value(output[0]["updated"][0]["id"].clone()).unwrap();
            let stored = support::read(&fixture.client, id).await.unwrap();
            assert_eq!(
                stored.body,
                format!("Verified independent condition {index}")
            );
        }
        assert!(pending.is_empty());
        assert_eq!(support::list(&fixture.client).await.unwrap().len(), 5);
        release.cancel();
        let mut ids = BTreeSet::new();
        for (index, (session, turn)) in sessions.iter().zip(&turns).enumerate() {
            let page = finished(&fixture.client, session.id, *turn).await;
            completed(&page, *turn);
            assert_eq!(page.approvals.len(), 1);
            assert_eq!(page.approvals[0].state, ApprovalState::Approved);
            assert_eq!(page.approvals[0].source, ApprovalSource::Storage);
            let output = results(&fixture.client, &page).await;
            assert_eq!(output.len(), 1, "{page:?}");
            assert!(output[0].get("isError").is_none(), "{output:?}");
            let id: MemoryId =
                serde_json::from_value(output[0]["updated"][0]["id"].clone()).unwrap();
            assert!(ids.insert(id));
            let stored = support::read(&fixture.client, id).await.unwrap();
            assert_eq!(
                stored.body,
                format!("Verified independent condition {index}")
            );
            assert_eq!(stored.summary.revision, 1);
        }
        rejected(&fixture.client, &prepared).await;
        assert_eq!(support::list(&fixture.client).await.unwrap().len(), 6);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
