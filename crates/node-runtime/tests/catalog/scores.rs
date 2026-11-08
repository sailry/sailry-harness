use super::*;

const BODY: &str = include_str!("../fixtures/model-scores.json");

fn response() -> Reply {
    Reply::Raw(format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: \"published\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{BODY}",
        BODY.len()
    ))
}

async fn reference(client: &Client) -> catalog::ModelRef {
    let mut provider = super::completion::provider();
    provider.models[0].id = "k2.8".into();
    provider.default_model = "k2.8".into();
    let provider = super::completion::save(client, provider).await;
    catalog::ModelRef {
        provider: provider.id,
        model: "k2.8".into(),
    }
}

async fn scores(client: &Client, reference: &catalog::ModelRef) -> catalog::Scores {
    let Output::ModelScores(scores) =
        execute(client, Command::ReadModelScores(vec![reference.clone()])).await
    else {
        panic!("model scores expected")
    };
    scores
}

#[tokio::test]
async fn refreshes_revalidates_and_restores() {
    let metadata = Server::start(ModelApi::Anthropic, |_| Reply::Json(data())).await;
    let count = AtomicUsize::new(0);
    let source = Server::start_with_request(ModelApi::Anthropic, move |request| {
        assert_eq!(request.path, "/scores.json");
        let index = count.fetch_add(1, Ordering::SeqCst);
        if index != 0 {
            assert_eq!(
                request.headers.get("if-none-match").map(String::as_str),
                Some("\"published\"")
            );
        }
        if index == 2 {
            Reply::Raw(
                "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .into(),
            )
        } else {
            response()
        }
    })
    .await;
    let fixture = Fixture::new(&metadata).await;
    fixture
        .node
        .set_model_scores_fixture(&format!("{}/scores.json", source.endpoint))
        .unwrap();
    let reference = reference(&fixture.client).await;
    let initial = refresh(&fixture.client).await;
    assert_eq!((initial.scores.revision, initial.scores.models), (1, 3));
    assert!(initial.scores.fault.is_none());
    assert_eq!(
        initial.scores.batch.as_deref(),
        Some("overall-f26004137653ddaeb3be2933")
    );
    let expected = scores(&fixture.client, &reference).await;
    assert_eq!(expected.scores.len(), 1);
    assert_eq!(expected.scores[0].efforts[0].overall, 62.75);
    assert_eq!(expected.scores[0].efforts[0].effort, Effort::High);
    for _ in 0..2 {
        let current = refresh(&fixture.client).await;
        assert_eq!(current.scores.revision, initial.scores.revision);
        assert_eq!(current.scores.updated_at_ms, initial.scores.updated_at_ms);
        assert!(current.scores.checked_at_ms >= initial.scores.checked_at_ms);
        assert_eq!(scores(&fixture.client, &reference).await, expected);
    }
    for request in source.requests.lock().unwrap().iter() {
        assert_eq!(request.method, "GET");
        assert!(request.body.is_empty());
        assert!(!request.headers.contains_key("authorization"));
        assert!(!request.headers.contains_key("cookie"));
        assert!(request.headers["user-agent"].starts_with("Sailry/"));
    }
    let path = fixture.node.profile().to_owned();
    let previous = status(&fixture.client).await;
    fixture.node.shutdown().await.unwrap();
    let reopened = Node::start_empty(path).await.unwrap();
    let client = Client::new(reopened.local());
    assert_eq!(status(&client).await, previous);
    assert_eq!(scores(&client, &reference).await, expected);
    fixture.controller.close().await.unwrap();
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_independent_source_results() {
    let count = AtomicUsize::new(0);
    let metadata = Server::start(ModelApi::Anthropic, move |_| {
        Reply::Json(if count.fetch_add(1, Ordering::SeqCst) == 1 {
            json!({})
        } else {
            data()
        })
    })
    .await;
    let count = AtomicUsize::new(0);
    let source = Server::start(ModelApi::Anthropic, move |_| {
        if matches!(count.fetch_add(1, Ordering::SeqCst), 0 | 3) {
            Reply::Json(json!({}))
        } else {
            response()
        }
    })
    .await;
    let fixture = Fixture::new(&metadata).await;
    fixture
        .node
        .set_model_scores_fixture(&source.endpoint)
        .unwrap();
    let reference = reference(&fixture.client).await;
    let first = refresh(&fixture.client).await;
    assert_eq!(first.revision, 1);
    assert!(first.fault.is_none());
    assert_eq!(first.scores.revision, 0);
    assert!(first.scores.fault.is_some());
    let second = refresh(&fixture.client).await;
    assert_eq!(second.revision, first.revision);
    assert_eq!(second.updated_at_ms, first.updated_at_ms);
    assert!(second.fault.is_some());
    assert_eq!(second.scores.revision, 1);
    assert!(second.scores.fault.is_none());
    assert_eq!(page(&fixture.client, query()).await.models.len(), 1);
    let third = refresh(&fixture.client).await;
    assert_eq!(third.revision, 2);
    assert!(third.fault.is_none() && third.scores.fault.is_none());
    let cached = scores(&fixture.client, &reference).await;
    let fourth = refresh(&fixture.client).await;
    assert!(fourth.fault.is_none() && fourth.scores.fault.is_some());
    assert_eq!(fourth.scores.revision, third.scores.revision);
    assert_eq!(fourth.scores.updated_at_ms, third.scores.updated_at_ms);
    assert_eq!(scores(&fixture.client, &reference).await, cached);
    fixture.close().await;
}

#[tokio::test]
async fn rolls_back_rejected_replacement() {
    let metadata = Server::start(ModelApi::Anthropic, |_| Reply::Json(data())).await;
    let count = AtomicUsize::new(0);
    let source = Server::start(ModelApi::Anthropic, move |_| {
        let mut body: Value = serde_json::from_str(BODY).unwrap();
        if count.fetch_add(1, Ordering::SeqCst) > 0 {
            body["snapshot"]["entries"][1]["overall_score"] = json!(63.5);
        }
        Reply::Json(body)
    })
    .await;
    let fixture = Fixture::new(&metadata).await;
    fixture
        .node
        .set_model_scores_fixture(&source.endpoint)
        .unwrap();
    let reference = reference(&fixture.client).await;
    let before = refresh(&fixture.client).await;
    let cached = scores(&fixture.client, &reference).await;
    fixture.node.with_storage(|db| Box::pin(async move {
        db.execute_batch("CREATE TRIGGER reject_scores BEFORE INSERT ON model_scores BEGIN SELECT RAISE(ABORT,'isolated score storage failure'); END;")
            .await.map_err(crate::database_fixture::fault)
    })).await.unwrap();
    assert!(
        fixture
            .client
            .execute(fixture.client.prepare(Command::RefreshModelCatalog))
            .await
            .is_err()
    );
    assert_eq!(scores(&fixture.client, &reference).await, cached);
    let current = status(&fixture.client).await;
    assert_eq!(current.scores, before.scores);
    assert_eq!(current.revision, before.revision + 1);
    fixture
        .node
        .with_storage(|db| {
            Box::pin(async move {
                db.execute_batch("DROP TRIGGER reject_scores")
                    .await
                    .map_err(crate::database_fixture::fault)
            })
        })
        .await
        .unwrap();
    let updated = refresh(&fixture.client).await;
    assert_eq!(updated.scores.revision, before.scores.revision + 1);
    assert_eq!(
        scores(&fixture.client, &reference).await.scores[0].efforts[0].overall,
        63.5
    );
    fixture.close().await;
}

#[tokio::test]
async fn shutdown_cancels_downloads() {
    let metadata = Server::start(ModelApi::Anthropic, |_| Reply::Json(data())).await;
    let source = Server::start(ModelApi::Anthropic, |_| Reply::Hold).await;
    let fixture = Fixture::new(&metadata).await;
    fixture
        .node
        .set_model_scores_fixture(&source.endpoint)
        .unwrap();
    let client = fixture.client.clone();
    let pending = tokio::spawn(async move {
        client
            .execute(client.prepare(Command::RefreshModelCatalog))
            .await
    });
    source.wait_requests(1).await;
    tokio::time::timeout(Duration::from_secs(5), fixture.node.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(pending.await.unwrap().is_err());
    source.wait_closed(1).await;
    fixture.controller.close().await.unwrap();
}

#[tokio::test]
async fn confines_downloads() {
    let metadata = Server::start(ModelApi::Anthropic, |_| Reply::Json(data())).await;
    let target = Server::start(ModelApi::Anthropic, |_| response()).await;
    let location = target.endpoint.clone();
    let count = AtomicUsize::new(0);
    let source = Server::start(ModelApi::Anthropic, move |_| {
        Reply::Raw(match count.fetch_add(1, Ordering::SeqCst) {
            0 => format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
            1 => "HTTP/1.1 200 OK\r\nContent-Length: 4194305\r\nConnection: close\r\n\r\n".into(),
            2 => format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n400001\r\n{}\r\n0\r\n\r\n", " ".repeat(4 * 1024 * 1024 + 1)),
            3 => "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
            _ => "HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        })
    }).await;
    let fixture = Fixture::new(&metadata).await;
    fixture
        .node
        .set_model_scores_fixture(&source.endpoint)
        .unwrap();
    for _ in 0..5 {
        let updated = refresh(&fixture.client).await;
        assert!(updated.fault.is_none());
        let fault = updated.scores.fault.unwrap();
        assert_eq!(fault.code, ErrorCode::Unavailable);
        assert!(!fault.message.contains(&target.endpoint));
        assert_eq!(updated.scores.revision, 0);
    }
    assert!(target.requests.lock().unwrap().is_empty());
    fixture.close().await;
}

#[tokio::test]
async fn disables_downloads_and_discards_pending_results() {
    let metadata = Server::start(ModelApi::Anthropic, |_| Reply::Json(data())).await;
    let release = Arc::new(tokio::sync::Notify::new());
    let barrier = release.clone();
    let count = AtomicUsize::new(0);
    let source = Server::start(ModelApi::Anthropic, move |_| {
        if count.fetch_add(1, Ordering::SeqCst) == 1 {
            Reply::Delayed(barrier.clone(), serde_json::from_str(BODY).unwrap())
        } else {
            response()
        }
    })
    .await;
    let fixture = Fixture::new(&metadata).await;
    fixture
        .node
        .set_model_scores_fixture(&source.endpoint)
        .unwrap();
    let reference = reference(&fixture.client).await;
    refresh(&fixture.client).await;
    let client = fixture.client.clone();
    let pending = tokio::spawn(async move { refresh(&client).await });
    source.wait_requests(2).await;
    let Output::CatalogStatus(disabled) = execute(
        &fixture.client,
        Command::SetModelScoresEnabled { enabled: false },
    )
    .await
    else {
        panic!("model catalog status expected")
    };
    assert!(!disabled.scores.enabled);
    assert_eq!(disabled.scores.models, 0);
    assert!(scores(&fixture.client, &reference).await.scores.is_empty());
    release.notify_one();
    assert!(!pending.await.unwrap().scores.enabled);
    refresh(&fixture.client).await;
    assert_eq!(source.requests.lock().unwrap().len(), 2);
    let counts = fixture.node.with_storage(|db| Box::pin(async move {
        let row = db.prepare("SELECT (SELECT count(*) FROM model_scores),(SELECT count(*) FROM model_score_aliases)")
            .await.map_err(crate::database_fixture::fault)?.query_row(()).await.map_err(crate::database_fixture::fault)?;
        Ok((row.get::<i64>(0).map_err(crate::database_fixture::fault)?, row.get::<i64>(1).map_err(crate::database_fixture::fault)?))
    })).await.unwrap();
    assert_eq!(counts, (0, 0));
    execute(
        &fixture.client,
        Command::SetModelScoresEnabled { enabled: true },
    )
    .await;
    let restored = refresh(&fixture.client).await;
    assert!(restored.scores.enabled && restored.scores.models == 3);
    assert_eq!(scores(&fixture.client, &reference).await.scores.len(), 1);
    fixture.close().await;
}
