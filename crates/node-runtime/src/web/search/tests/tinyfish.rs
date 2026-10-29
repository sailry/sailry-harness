use super::*;

#[tokio::test]
async fn pauses_authentication_and_rate_limits() {
    for status in [401, 429] {
        let server = Server::start().await;
        server.respond("/tinyfish", vec![Reply::Status(status, None)]);
        let owner = Arc::new(Searcher::default());
        let mut plan = plan(&[Driver::Tinyfish]);
        plan.settings.fallback = false;
        owner.configure(plan.settings.clone());
        owner.fixture(Driver::Tinyfish, &server.url(Driver::Tinyfish));
        let error = search(&owner, plan.clone()).await.unwrap_err();
        assert!(!error.message.contains("private-response"));
        assert_eq!(
            error.code,
            if status == 401 {
                ErrorCode::PermissionDenied
            } else {
                ErrorCode::Unavailable
            }
        );
        assert!(
            owner
                .cooldown(Driver::Tinyfish, plan.settings.revision)
                .is_some()
        );
        assert_eq!(
            search(&owner, plan).await.unwrap_err().code,
            ErrorCode::Unavailable
        );
        assert_eq!(server.count("/tinyfish"), 1);
    }
}

#[test]
fn follows_exa_in_auto_selection() {
    let mut plan = plan(&[Driver::Exa, Driver::Tinyfish, Driver::Tavily]);
    plan.settings.driver = Some(Selection::Auto);
    assert_eq!(
        candidates(&plan.settings),
        [Driver::Exa, Driver::Tinyfish, Driver::Tavily]
    );
    plan.settings
        .providers
        .get_mut(&Driver::Tinyfish)
        .unwrap()
        .key_configured = false;
    assert_eq!(candidates(&plan.settings), [Driver::Exa, Driver::Tavily]);
}
