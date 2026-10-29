use super::*;

#[tokio::test]
async fn reuses_pages_and_bypasses_fresh_requests() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let plan = plan(&[Driver::Tavily]);
    owner.configure(plan.settings.clone());
    owner.fixture(Driver::Tavily, &server.url(Driver::Tavily));
    let first = search(&owner, plan.clone()).await.unwrap();
    server.respond(
        "/tavily",
        vec![Reply::Json(serde_json::json!({"bad":"response"}))],
    );
    assert_eq!(search(&owner, plan.clone()).await.unwrap(), first);
    assert_eq!(server.count("/tavily"), 1);
    assert_eq!(
        fresh(&owner, plan.clone()).await.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(server.count("/tavily"), 2);
    assert_eq!(search(&owner, plan).await.unwrap(), first);
    assert_eq!(server.count("/tavily"), 2);
}

#[tokio::test]
async fn invalidates_changed_settings() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let mut plan = plan(&[Driver::Tavily]);
    owner.configure(plan.settings.clone());
    owner.fixture(Driver::Tavily, &server.url(Driver::Tavily));
    search(&owner, plan.clone()).await.unwrap();
    owner.configure(plan.settings.clone());
    search(&owner, plan.clone()).await.unwrap();
    assert_eq!(server.count("/tavily"), 1);
    plan.settings.revision += 1;
    owner.configure(plan.settings.clone());
    search(&owner, plan.clone()).await.unwrap();
    assert_eq!(server.count("/tavily"), 2);
    plan.settings.fallback = !plan.settings.fallback;
    owner.configure(plan.settings.clone());
    search(&owner, plan).await.unwrap();
    assert_eq!(server.count("/tavily"), 3);
}

#[tokio::test]
async fn isolates_stale_inflight_settings() {
    for revision_only in [false, true] {
        let server = Server::start().await;
        let owner = Arc::new(Searcher::default());
        let captured = plan(&[Driver::Tavily]);
        owner.configure(captured.settings.clone());
        owner.fixture(Driver::Tavily, &server.url(Driver::Tavily));
        let release = CancellationToken::new();
        server.releases.lock().unwrap().insert(
            "/tavily".into(),
            std::collections::VecDeque::from([release.clone()]),
        );
        let pending = {
            let owner = owner.clone();
            let plan = captured.clone();
            tokio::spawn(async move { search(&owner, plan).await })
        };
        tokio::time::timeout(Duration::from_secs(5), server.held.cancelled())
            .await
            .unwrap();
        let mut current = captured.clone();
        if revision_only {
            current.settings.revision += 1;
        } else {
            current.settings.fallback = !current.settings.fallback;
        }
        owner.configure(current.settings.clone());
        release.cancel();
        pending.await.unwrap().unwrap();
        search(&owner, current.clone()).await.unwrap();
        assert_eq!(server.count("/tavily"), 2);
        search(&owner, captured).await.unwrap();
        assert_eq!(server.count("/tavily"), 3);
        search(&owner, current).await.unwrap();
        assert_eq!(server.count("/tavily"), 3);
    }
}

#[tokio::test]
async fn honors_cancellation_before_cache_lookup() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let plan = plan(&[Driver::Tavily]);
    owner.configure(plan.settings.clone());
    owner.fixture(Driver::Tavily, &server.url(Driver::Tavily));
    search(&owner, plan.clone()).await.unwrap();
    let stop = CancellationToken::new();
    stop.cancel();
    let error = owner
        .search(NodeId([1; 32]), RequestId::new(), plan, &input(), &stop)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(server.count("/tavily"), 1);
    assert!(owner.active.lock().unwrap().is_empty());
}
