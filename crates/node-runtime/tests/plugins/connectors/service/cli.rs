use super::*;
#[path = "../../../support/github.rs"]
mod source;

fn stored(node: &Node, id: connector::Id) -> Vec<u8> {
    rusqlite::Connection::open_with_flags(
        node.profile().join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
    .query_row(
        "SELECT body FROM plugin_connections WHERE id=?1",
        [id.to_string()],
        |row| row.get(0),
    )
    .unwrap()
}

#[tokio::test]
async fn bounds_cache_and_preserves_temporary_failures() {
    for remote in [false, true] {
        let programs = tempfile::tempdir().unwrap();
        let cli = source::Cli::new(programs.path());
        let (directory, node, controller, client, worktree) =
            support::empty_with_environment(remote, cli.source()).await;
        let server = server::Server::start().await;
        node.set_connector_fixture(ORIGIN, &server.origin);
        let manifest: Value =
            serde_json::from_str(include_str!("../../../../../../plugins/github/plugin.json"))
                .unwrap();
        let mut auth =
            manifest["extensions"]["dev.sailry.platform"]["connectors"][0]["auth"][1].clone();
        auth["timeout_ms"] = json!(1000);
        let mut declared = declaration();
        declared["auth"] = json!([auth]);
        declared["fields"] = json!([]);
        configure(&directory.path().join("source/package"), declared);
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let Output::ConnectorLogin(attempt) = client
            .execute(client.prepare(Command::StartConnectorLogin {
                package: package.clone(),
                connector: "service".into(),
                connection: None,
                expected_revision: None,
                method: Kind::LocalCli,
                input: Input::default(),
            }))
            .await
            .unwrap()
        else {
            panic!("connector login expected")
        };
        let mut login = client.subscribe_connector_login(attempt.id).await.unwrap();
        let row = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let Update::ConnectorLogin(update) = login.next().await.unwrap() else {
                    panic!("login update expected")
                };
                if let LoginState::Connected(row) = update.state {
                    break row;
                }
                assert!(update.state.active(), "{:?}", update.state);
            }
        })
        .await
        .unwrap();
        let scope = context(package, worktree);
        let call = |path: &str| {
            client
                .prepare(request(path, row.id))
                .with_plugin(scope.clone())
        };
        assert!(client.execute(call("/ok")).await.is_ok());
        let count = cli.calls("token");
        assert!(client.execute(call("/other")).await.is_ok());
        assert_eq!(cli.calls("token"), count);
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(client.execute(call("/ok")).await.is_ok());
        assert_eq!(cli.calls("token"), count + 1);

        tokio::time::sleep(Duration::from_millis(1100)).await;
        fs::write(cli.home().join("gh-fail"), "").unwrap();
        let before = stored(&node, row.id);
        assert!(client.execute(call("/ok")).await.is_err());
        assert_eq!(stored(&node, row.id), before);
        fs::remove_file(cli.home().join("gh-fail")).unwrap();
        assert!(client.execute(call("/ok")).await.is_ok());

        tokio::time::sleep(Duration::from_millis(1100)).await;
        fs::write(cli.home().join("gh-hold"), "").unwrap();
        let before = stored(&node, row.id);
        assert!(client.execute(call("/ok")).await.is_err());
        assert!(cli.home().join("gh-entered").exists());
        assert_eq!(stored(&node, row.id), before);
        fs::remove_file(cli.home().join("gh-hold")).unwrap();
        assert!(client.execute(call("/ok")).await.is_ok());

        let before = server.requests.lock().unwrap().len();
        let count = cli.calls("token");
        let Output::PluginHttp(response) = client.execute(call("/unauthorized")).await.unwrap()
        else {
            panic!("HTTP response expected")
        };
        assert_eq!(response.status, 401);
        assert_eq!(server.requests.lock().unwrap().len() - before, 2);
        assert_eq!(cli.calls("token"), count + 1);
        let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
            panic!("connections expected")
        };
        assert_eq!(rows[0].state, State::Expired);
        assert!(
            !serde_json::to_string(&rows)
                .unwrap()
                .contains(source::TOKEN)
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
