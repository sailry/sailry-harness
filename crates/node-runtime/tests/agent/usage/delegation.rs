use super::*;
use crate::delegation::fixture::Fixture;
use sailry_client::conversation::Projection;
use sailry_protocol::usage::Metrics;
use serde_json::json;

async fn statistics(client: &Client, session: SessionId) -> Statistics {
    let mut updates = client.subscribe_conversation(session).await.unwrap();
    let Update::ConversationSnapshot(snapshot) = updates.next().await.unwrap() else {
        panic!("conversation snapshot expected")
    };
    snapshot.statistics
}

async fn settled(client: &Client, session: SessionId) -> Page {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut updates = client.subscribe_conversation(session).await.unwrap();
        let mut projection = Projection::new(client.target(), session, 0);
        loop {
            projection.apply(0, updates.next().await.unwrap()).unwrap();
            let Some(snapshot) = projection.snapshot() else {
                continue;
            };
            let page = &snapshot.page;
            let terminal = |run: &sailry_protocol::conversation::Run| {
                !matches!(
                    run.status,
                    Status::Queued | Status::Running | Status::Stopping
                )
            };
            if !page.background.is_empty()
                && page.queue.items.is_empty()
                && page.runs.iter().all(terminal)
                && page.background.iter().all(|task| {
                    use sailry_protocol::conversation::background::State;
                    matches!(task.state, State::Consumed | State::Dismissed)
                        || task.state == State::Delivered
                            && task.delivery.is_some_and(|turn| {
                                page.runs
                                    .iter()
                                    .any(|run| run.turn == turn && terminal(run))
                            })
                })
            {
                return page.as_ref().clone();
            }
        }
    })
    .await
    .expect("background completion did not settle")
}

#[tokio::test]
async fn publishes_child_usage_to_parent() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let parent = Server::tools(vec![
            (
                crate::agent_support::plugin_tool("delegation", "spawn_agent"),
                json!({"role":"review","task":"Measure the child","background":true}),
            ),
            (
                crate::agent_support::plugin_tool("files", "write_file"),
                json!({"path":"held.txt","text":"Await approval","expected_revision":null}),
            ),
        ])
        .await;
        let child = Server::markdown_held("Measured child answer".into(), release.clone()).await;
        let mut fixture = Fixture::start(remote, parent, child).await;
        let mut updates = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let mut projection = Projection::new(fixture.client.target(), fixture.session.id, 0);
        projection.apply(0, updates.next().await.unwrap()).unwrap();
        let turn = fixture.push("Measure delegated usage").await;
        let (_, approval) = crate::approvals::pending(&fixture.client, fixture.session.id).await;
        fixture.child.wait_count(1).await;
        let child = fixture
            .snapshot()
            .await
            .sessions
            .into_iter()
            .find(|session| {
                session
                    .delegation
                    .as_ref()
                    .is_some_and(|origin| origin.session == fixture.session.id)
            })
            .unwrap();
        let before = statistics(&fixture.client, fixture.session.id).await;
        assert_eq!(before.responses, 2);
        assert_eq!(before.delegated, Metrics::default());
        release.cancel();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                projection.apply(0, updates.next().await.unwrap()).unwrap();
                if projection
                    .snapshot()
                    .unwrap()
                    .statistics
                    .delegated
                    .responses
                    == 1
                {
                    break;
                }
            }
        })
        .await
        .expect("parent delegated statistics deadline");
        let live = projection.snapshot().unwrap().statistics.clone();
        assert_eq!(live.responses, before.responses);
        assert_eq!(live.usage, before.usage);
        assert_eq!(live.context_tokens, before.context_tokens);
        assert_eq!(live.generation, before.generation);
        assert_eq!(live.delegated.tokens.as_ref().unwrap().input, 12);
        assert_eq!(fixture.parent.requests.lock().unwrap().len(), 2);
        execute(
            &fixture.client,
            Command::ResolveApproval {
                session: fixture.session.id,
                approval: approval.id,
                decision: Decision::Deny,
            },
        )
        .await;
        let completed = finished(&fixture.client, fixture.session.id, turn.id).await;
        let own: Vec<_> = completed
            .entries
            .iter()
            .filter(|entry| entry.turn == turn.id)
            .filter_map(|entry| entry.usage.as_ref())
            .collect();
        assert_eq!(own.len(), 3);
        assert_eq!(own.iter().map(|usage| usage.input).sum::<u64>(), 36);
        assert_eq!(own.iter().map(|usage| usage.output).sum::<u64>(), 12);
        let completed = settled(&fixture.client, fixture.session.id).await;
        let notices = completed.runs.iter().filter(|run| run.notice).count() as u64;
        assert!(notices <= 1);
        assert!(
            completed
                .runs
                .iter()
                .filter(|run| run.notice)
                .all(|run| run.status == Status::Completed)
        );
        let responses = 3 + notices;
        assert_eq!(
            fixture.parent.requests.lock().unwrap().len() as u64,
            responses
        );
        let parent = statistics(&fixture.client, fixture.session.id).await;
        let child = statistics(&fixture.client, child.id).await;
        assert_eq!(parent.responses, responses);
        assert_eq!(parent.context_tokens, Some(16));
        assert_eq!(parent.generation.as_ref().unwrap().responses, responses);
        assert_eq!(
            parent.generation.as_ref().unwrap().output_tokens,
            responses * 4
        );
        assert_eq!(
            parent.delegated,
            Metrics {
                responses: child.responses,
                tokens: child.usage,
                cost: child.cost,
                generation: child.generation,
            }
        );
        let now = chrono::Utc::now().timestamp_millis();
        let query = Query {
            start_ms: now - 86_400_000,
            end_ms: now + 86_400_000,
            dimension: Dimension::Model,
            utc_offset_minutes: 0,
            projects: vec![],
            worktrees: vec![],
            providers: vec![],
            models: vec![],
            before: None,
        };
        let report = fixture.client.read_usage(query.clone()).await.unwrap();
        assert_eq!(
            report.totals.responses,
            parent.responses + parent.delegated.responses
        );
        assert_eq!(
            report.totals.tokens.as_ref().unwrap().input,
            parent.usage.as_ref().unwrap().input + parent.delegated.tokens.as_ref().unwrap().input
        );
        assert_eq!(report.requests.items.len() as u64, responses + 1);
        drop(updates);
        let id = fixture.node.id();
        fixture.node.shutdown().await.unwrap();
        assert_eq!(
            fixture.parent.requests.lock().unwrap().len() as u64,
            responses
        );
        fixture.controller.handle().disconnect(id).await;
        let profile = fixture.directory.path().join("node");
        let (database, mut db) =
            crate::database_fixture::open(&profile.join("storage/node.turso"), false).await;
        let transaction = db.transaction().await.unwrap();
        for model in ["fixture-a", "fixture-child"] {
            transaction
                .execute(
                    "INSERT INTO model_catalog(provider,id,body) VALUES('openai',?1,?2)",
                    turso::params![
                        model,
                        serde_json::to_vec(&json!({
                            "id":model,"cost":{"input":1,"output":1,"cache_read":1}
                        }))
                        .unwrap()
                    ],
                )
                .await
                .unwrap();
        }
        transaction.commit().await.unwrap();
        drop(db);
        drop(database);
        fixture.node = Node::start_empty(&profile).await.unwrap();
        fixture.client = Client::new(if remote {
            fixture
                .controller
                .handle()
                .remote(fixture.node.link().address())
        } else {
            fixture.node.local()
        });
        let restored = statistics(&fixture.client, fixture.session.id).await;
        assert_eq!(
            fixture.parent.requests.lock().unwrap().len() as u64,
            responses
        );
        assert_eq!(restored.usage, parent.usage);
        assert_eq!(restored.generation, parent.generation);
        assert_eq!(restored.context_tokens, parent.context_tokens);
        assert_eq!(restored.delegated.tokens, parent.delegated.tokens);
        assert_eq!(restored.cost.as_ref().unwrap().usd_micros, responses * 16);
        assert_eq!(restored.delegated.cost.as_ref().unwrap().usd_micros, 16);
        let report = fixture.client.read_usage(query).await.unwrap();
        assert_eq!(
            report.totals.cost.as_ref().unwrap().usd_micros,
            (responses + 1) * 16
        );
        assert_eq!(
            report.totals.cost.as_ref().unwrap().responses,
            responses + 1
        );
        fixture.close().await;
    }
}
