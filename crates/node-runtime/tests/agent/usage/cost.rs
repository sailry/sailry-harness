use super::*;
#[path = "../../support/pricing.rs"]
mod pricing;

#[tokio::test]
async fn restores_cost_sources() {
    for (remote, catalog) in [(false, false), (true, false), (false, true), (true, true)] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        let server = Server::start(false).await;
        let writer = Client::new(node.local());
        let (session, _) = configured(&writer, &server.endpoint, &root).await;
        for _ in 0..2 {
            let Output::QueuedTurn(turn) = execute(
                &writer,
                Command::SubmitTurn {
                    session: session.id,
                    expected_revision: 1,
                    message: "Report usage".into(),
                },
            )
            .await
            else {
                panic!("turn expected")
            };
            finished(&writer, session.id, turn.id).await;
        }
        node.shutdown().await.unwrap();
        pricing::seed(&profile);
        if catalog {
            seed_catalog(&profile, &session.config.model);
        }
        let node = Node::start(&profile).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Arc::new(Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        }));
        let now = chrono::Utc::now().timestamp_millis();
        let query = Query {
            start_ms: now - 86_400_000,
            end_ms: now + 86_400_000,
            dimension: Dimension::Model,
            projects: vec![],
            worktrees: vec![],
            providers: vec![],
            models: vec![],
            before: None,
        };
        let (updates, mut view) = watch::channel(View::default());
        let (refresh, requests) = mpsc::channel(1);
        let stop = CancellationToken::new();
        let worker = tokio::spawn({
            let client = client.clone();
            let stop = stop.clone();
            let query = query.clone();
            async move { client.watch_usage(query, updates, stop, requests).await }
        });
        observed(&mut view, |view| {
            view.connected && !view.refreshing && view.report.is_some()
        })
        .await;
        let report = view.borrow().report.clone().unwrap();
        assert_eq!(report.totals.responses, 2);
        let generation = report.totals.generation.as_ref().unwrap();
        assert_eq!(generation.responses, 2);
        assert_eq!(generation.output_tokens, 8);
        assert!(generation.elapsed_us > 0);
        let cost = report.totals.cost.as_ref().unwrap();
        if catalog {
            let tokens = report.totals.tokens.as_ref().unwrap();
            assert_eq!(cost.responses, 2);
            assert_eq!(cost.usd_micros, tokens.input + tokens.output);
            assert_eq!(
                cost.breakdown,
                Some(sailry_protocol::usage::CostBreakdown {
                    input: tokens.input - tokens.cached_input,
                    output: tokens.output,
                    cache_read: tokens.cached_input,
                    cache_write: 0,
                })
            );
        } else {
            assert_eq!(cost.responses, 1);
            assert_eq!(cost.usd_micros, 125000);
            assert_eq!(cost.breakdown, None);
        }
        assert_eq!(report.groups[0].metrics.cost, report.totals.cost);
        controller.handle().disconnect(node.id()).await;
        refresh.send(()).await.unwrap();
        observed(&mut view, |view| {
            view.connected && !view.refreshing && view.report.as_deref() == Some(&report)
        })
        .await;
        assert_eq!(client.read_usage(query).await.unwrap(), *report);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        stop.cancel();
        worker.await.unwrap().unwrap();
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

fn seed_catalog(profile: &std::path::Path, model: &str) {
    let db = rusqlite::Connection::open_with_flags(
        profile.join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .unwrap();
    // Only the stopped fixture's frozen metadata changes; no official API is called.
    db.execute(
        "UPDATE agent_runs SET provider=CAST(json_set(provider,'$.endpoint','https://api.openai.com/v1') AS BLOB)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO model_catalog(provider,id,body) VALUES('openai',?1,?2)",
        rusqlite::params![
            model,
            serde_json::to_vec(&serde_json::json!({
                "id": model,
                "cost": {"input":1,"output":1,"cache_read":1,"cache_write":1}
            }))
            .unwrap()
        ],
    )
    .unwrap();
}
