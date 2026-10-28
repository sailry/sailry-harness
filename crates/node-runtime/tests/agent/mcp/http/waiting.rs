use super::*;

async fn rules(node: &Node) -> i64 {
    node.with_storage(|db| {
        Box::pin(async move {
            db.prepare("SELECT count(*) FROM agent_approval_rules")
                .await
                .map_err(crate::database_fixture::fault)?
                .query_row(())
                .await
                .map_err(crate::database_fixture::fault)?
                .get::<i64>(0)
                .map_err(crate::database_fixture::fault)
        })
    })
    .await
    .unwrap()
}

async fn continued(model: &Server) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while model.requests.lock().unwrap().len() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("model did not receive the settled tool result");
}

#[tokio::test]
async fn rejects_decisions_after_timeout() {
    for remote in [false, true] {
        let http = Http::start(Reply::Normal).await;
        let release = Arc::new(tokio::sync::Notify::new());
        let model = Server::held_after_tool(
            vec![(alias("remote", "write"), json!({"value":"must not run"}))],
            release.clone(),
        )
        .await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        fixture
            .node
            .set_approval_timeout_fixture(Duration::from_millis(500));
        package(
            &fixture.root,
            json!({"remote":{"type":"streamable-http","url":http.endpoint}}),
            "approval timeout",
        );
        install(&fixture, 0).await;
        let (_, turn) = submit(&fixture, false).await;
        let (_, approval) = crate::approvals::pending(&fixture.client, fixture.session.id).await;
        assert!(approval.can_remember);
        continued(&model).await;

        // The SDK has denied this call, but the following model response is still held.
        let page = crate::approvals::rules::observe(&fixture.client, fixture.session.id, |page| {
            page.approvals
                .iter()
                .any(|entry| entry.id == approval.id && entry.state == ApprovalState::Cancelled)
        })
        .await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Running
        );
        assert!(
            page.approvals
                .iter()
                .all(|approval| approval.state != ApprovalState::Pending)
        );
        assert_eq!(
            page.approvals
                .iter()
                .find(|entry| entry.id == approval.id)
                .unwrap()
                .state,
            ApprovalState::Cancelled
        );
        for decision in [Decision::Approve, Decision::ApproveSession] {
            let error = fixture
                .client
                .execute(fixture.client.prepare(Command::ResolveApproval {
                    session: fixture.session.id,
                    approval: approval.id,
                    decision,
                }))
                .await
                .unwrap_err();
            assert_eq!(
                error.code,
                ErrorCode::Conflict,
                "remote={remote}: {error:?}"
            );
        }
        assert_eq!(rules(&fixture.node).await, 0);
        assert_eq!(http.count("tools/call"), 0);
        assert_eq!(http.effects.load(Ordering::SeqCst), 0);
        release.notify_one();
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        let run = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
        assert_eq!(run.status, Status::Completed);
        assert!(run.error.is_none());
        assert_eq!(http.count("tools/call"), 0);
        assert_eq!(rules(&fixture.node).await, 0);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn closes_cancelled_waits() {
    for remote in [false, true] {
        let http = Http::start(Reply::Normal).await;
        let model = Server::tools(vec![(
            alias("remote", "write"),
            json!({"value":"cancelled"}),
        )])
        .await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        fixture
            .node
            .set_approval_timeout_fixture(Duration::from_secs(5));
        package(
            &fixture.root,
            json!({"remote":{"type":"streamable-http","url":http.endpoint}}),
            "approval cancellation",
        );
        install(&fixture, 0).await;
        let (_, turn) = submit(&fixture, false).await;
        let (_, approval) = crate::approvals::pending(&fixture.client, fixture.session.id).await;
        execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Cancelled
        );
        assert_eq!(
            page.approvals
                .iter()
                .find(|entry| entry.id == approval.id)
                .unwrap()
                .state,
            ApprovalState::Cancelled
        );
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ResolveApproval {
                    session: fixture.session.id,
                    approval: approval.id,
                    decision: Decision::ApproveSession,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(rules(&fixture.node).await, 0);
        assert_eq!(http.count("tools/call"), 0);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn resumes_live_approvals() {
    for remote in [false, true] {
        let http = Http::start(Reply::Normal).await;
        let model = Server::tools(vec![(
            alias("remote", "write"),
            json!({"value":"approved"}),
        )])
        .await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        fixture
            .node
            .set_approval_timeout_fixture(Duration::from_secs(5));
        package(
            &fixture.root,
            json!({"remote":{"type":"streamable-http","url":http.endpoint}}),
            "live approval",
        );
        install(&fixture, 0).await;
        let (_, turn) = submit(&fixture, false).await;
        let (_, approval) = crate::approvals::pending(&fixture.client, fixture.session.id).await;
        execute(
            &fixture.client,
            Command::ResolveApproval {
                session: fixture.session.id,
                approval: approval.id,
                decision: Decision::ApproveSession,
            },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(
            page.approvals
                .iter()
                .find(|entry| entry.id == approval.id)
                .unwrap()
                .state,
            ApprovalState::Approved
        );
        assert_eq!(rules(&fixture.node).await, 1);
        assert_eq!(http.count("tools/call"), 1);
        assert_eq!(http.effects.load(Ordering::SeqCst), 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn orders_expiry_and_commit() {
    for remote in [false, true] {
        for (after_claim, fail) in [(false, false), (true, false), (true, true)] {
            let http = Http::start(Reply::Normal).await;
            let release_model = Arc::new(tokio::sync::Notify::new());
            let model = Server::held_after_tool(
                vec![(alias("remote", "write"), json!({"value":"commit order"}))],
                release_model.clone(),
            )
            .await;
            let fixture = process::Fixture::packages(remote, &model, &[]).await;
            fixture
                .node
                .set_approval_timeout_fixture(Duration::from_secs(2));
            package(
                &fixture.root,
                json!({"remote":{"type":"streamable-http","url":http.endpoint}}),
                "approval commit order",
            );
            install(&fixture, 0).await;
            let (entered, release) = fixture
                .node
                .hold_approval_commit_fixture(after_claim, fail)
                .await
                .unwrap();
            let (_, turn) = submit(&fixture, false).await;
            let (_, approval) =
                crate::approvals::pending(&fixture.client, fixture.session.id).await;
            let request = fixture.client.prepare(Command::ResolveApproval {
                session: fixture.session.id,
                approval: approval.id,
                decision: Decision::ApproveSession,
            });
            let resolving = fixture.client.execute(request);
            tokio::pin!(resolving);
            tokio::select! {
                _ = entered.notified() => {},
                result = &mut resolving => panic!("approval skipped commit barrier: remote={remote}, after_claim={after_claim}, fail={fail}: {result:?}"),
            }
            tokio::time::timeout(
                Duration::from_secs(5),
                fixture.node.wait_approval_expiry_fixture(),
            )
            .await
            .expect("approval did not request expiry");
            release.send(()).unwrap();
            let committed = after_claim && !fail;
            let result = resolving.await;
            if committed {
                assert!(
                    matches!(result.unwrap(), Output::Approval(entry) if entry.state == ApprovalState::Approved)
                );
            } else {
                let error = result.unwrap_err();
                assert_eq!(
                    error.code,
                    if fail {
                        ErrorCode::Internal
                    } else {
                        ErrorCode::Conflict
                    }
                );
            }
            continued(&model).await;
            let state = if committed {
                ApprovalState::Approved
            } else {
                ApprovalState::Cancelled
            };
            let page =
                crate::approvals::rules::observe(&fixture.client, fixture.session.id, |page| {
                    page.approvals
                        .iter()
                        .any(|entry| entry.id == approval.id && entry.state == state)
                })
                .await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    .unwrap()
                    .status,
                Status::Running
            );
            assert!(
                page.approvals
                    .iter()
                    .all(|entry| entry.state != ApprovalState::Pending)
            );
            let count = usize::from(committed);
            assert_eq!(rules(&fixture.node).await, count as i64);
            assert_eq!(http.count("tools/call"), count);
            assert_eq!(http.effects.load(Ordering::SeqCst), count);
            release_model.notify_one();
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            let run = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
            assert_eq!(run.status, Status::Completed);
            assert!(run.error.is_none());
            assert_eq!(rules(&fixture.node).await, count as i64);
            assert_eq!(http.count("tools/call"), count);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
