use super::*;
use std::sync::atomic::Ordering;

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
async fn expires_rejected_credentials_and_allows_rechecking() {
    for remote in [false, true] {
        let server = server::Server::respond(Some("{}".into()), None).await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture(ORIGIN, &server.origin);
        let mut declared = declaration();
        declared["health"] = json!({"request":format!("{ORIGIN}/health"),"ok":[200]});
        configure(&directory.path().join("source/package"), declared);
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let row = connected(&client, package, None, None, secret()).await;
        let before: Value = serde_json::from_slice(&stored(&node, row.id)).unwrap();
        server.status.store(401, Ordering::SeqCst);
        let request = client.prepare(Command::ProbeConnection { connection: row.id });
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::Expired
        );
        let after: Value = serde_json::from_slice(&stored(&node, row.id)).unwrap();
        assert_eq!(after["state"], "expired");
        for field in ["fields", "secrets", "auth_revision", "revision"] {
            assert_eq!(after[field], before[field], "{remote}/{field}");
        }
        let bytes = stored(&node, row.id);
        server.status.store(503, Ordering::SeqCst);
        assert_eq!(
            client
                .execute(client.prepare(Command::ProbeConnection { connection: row.id }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        assert_eq!(stored(&node, row.id), bytes);
        server.status.store(0, Ordering::SeqCst);
        let Output::Connection(row) =
            execute(&client, Command::ProbeConnection { connection: row.id }).await
        else {
            panic!("connection expected")
        };
        assert_eq!(row.state, State::Connected);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn recovers_failed_and_expired_health_checks() {
    for remote in [false, true] {
        let server = server::Server::respond(Some("{}".into()), None).await;
        let held = server::Server::respond(Some("{}".into()), Some("/health".into())).await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture(ORIGIN, &server.origin);
        let mut declared = declaration();
        declared["health"] = json!({"request":format!("{ORIGIN}/health"),"ok":[200]});
        configure(&directory.path().join("source/package"), declared);
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let row = connected(&client, package.clone(), None, None, secret()).await;
        let before = stored(&node, row.id);
        node.set_connector_fixture(ORIGIN, &held.origin);
        let error = client
            .execute(client.prepare(Command::ProbeConnection { connection: row.id }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(held.held.is_cancelled());
        assert_eq!(stored(&node, row.id), before);
        assert!(
            client
                .execute(
                    client
                        .prepare(request("/ok", row.id))
                        .with_plugin(context(package.clone(), worktree))
                )
                .await
                .is_ok()
        );
        node.set_connector_fixture(ORIGIN, &server.origin);
        let profile = node.profile().to_owned();
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        let database = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let mut failed: Value = serde_json::from_slice(&before).unwrap();
        failed["state"] = json!("failed");
        database
            .execute(
                "UPDATE plugin_connections SET body=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_vec(&failed).unwrap(), row.id.to_string()],
            )
            .unwrap();
        database.close().unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        node.set_connector_fixture(ORIGIN, &server.origin);
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let scope = context(package, worktree);
        assert!(
            client
                .execute(
                    client
                        .prepare(request("/ok", row.id))
                        .with_plugin(scope.clone())
                )
                .await
                .is_ok()
        );
        let Output::Connection(recovered) =
            execute(&client, Command::ProbeConnection { connection: row.id }).await
        else {
            panic!("connection expected")
        };
        assert_eq!(recovered.state, State::Connected);
        let Output::PluginHttp(response) = client
            .execute(
                client
                    .prepare(request("/unauthorized", row.id))
                    .with_plugin(scope.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("HTTP result expected")
        };
        assert_eq!(response.status, 401);
        let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
            panic!("connections expected")
        };
        assert_eq!(rows[0].state, State::Expired);
        let Output::Connection(recovered) =
            execute(&client, Command::ProbeConnection { connection: row.id }).await
        else {
            panic!("connection expected")
        };
        assert_eq!(recovered.state, State::Connected);
        assert!(
            client
                .execute(client.prepare(request("/ok", row.id)).with_plugin(scope))
                .await
                .is_ok()
        );
        execute(
            &client,
            Command::SetConnectionEnabled {
                connection: row.id,
                expected_revision: recovered.revision,
                enabled: false,
            },
        )
        .await;
        let before = server.requests.lock().unwrap().len();
        assert_eq!(
            client
                .execute(client.prepare(Command::ProbeConnection { connection: row.id }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(server.requests.lock().unwrap().len(), before);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
