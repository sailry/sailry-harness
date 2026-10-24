use super::*;
use std::sync::atomic::Ordering;

async fn stored(node: &Node, id: connector::Id) -> Vec<u8> {
    node.with_storage(move |db| {
        Box::pin(async move {
            db.prepare("SELECT body FROM plugin_connections WHERE id=?1")
                .await
                .map_err(crate::plugins::database_fixture::fault)?
                .query_row([id.to_string()])
                .await
                .map_err(crate::plugins::database_fixture::fault)?
                .get(0)
                .map_err(crate::plugins::database_fixture::fault)
        })
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn sends_every_scoped_header() {
    const FIRST: &str = "first-private-health-token";
    const SECOND: &str = "second-private-health-token";
    for remote in [false, true] {
        let headers = vec![
            ("Authorization".into(), format!("Bearer {SECOND}")),
            ("X-Client-Key".into(), FIRST.into()),
        ];
        let server = server::Server::build(
            Some(json!({"name":"Fixture","id":"fixture-account"}).to_string()),
            None,
            headers.clone(),
        )
        .await;
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        node.set_connector_fixture(ORIGIN, &server.origin);
        let mut declared = declaration();
        declared["origins"] = json!([ORIGIN, "https://other.example.com"]);
        declared["auth"][0]["fields"] = json!(["alpha", "beta"]);
        declared["fields"] = json!([
            {"name":"alpha","title":"Alpha","secret":true},
            {"name":"beta","title":"Beta","secret":true},
            {"name":"unused","title":"Unused","secret":true,"optional":true}
        ]);
        declared["inject"] = json!([
            {"origin":ORIGIN,"header":"X-Optional","field":"unused"},
            {"origin":ORIGIN,"header":"Authorization","prefix":"Bearer ","field":"beta"},
            {"origin":ORIGIN,"header":"X-Client-Key","field":"alpha"},
            {"origin":"https://other.example.com","header":"X-Other-Key","field":"alpha"}
        ]);
        declared["identity"] =
            json!({"request":format!("{ORIGIN}/identity"),"name":"/name","id":"/id"});
        declared["health"] = json!({"request":format!("{ORIGIN}/health"),"ok":[200]});
        configure(&directory.path().join("source/package"), declared);
        let package = info(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let row = connected(
            &client,
            package,
            None,
            None,
            Input {
                secrets: BTreeMap::from([
                    ("alpha".into(), Secret::new(FIRST.into())),
                    ("beta".into(), Secret::new(SECOND.into())),
                ]),
                ..Default::default()
            },
        )
        .await;
        assert_eq!(row.state, State::Connected);
        assert_eq!(row.identity.as_ref().unwrap().id, "fixture-account");
        let before: Value = serde_json::from_slice(&stored(&node, row.id).await).unwrap();
        let output = execute(&client, Command::ProbeConnection { connection: row.id }).await;
        let Output::Connection(checked) = &output else {
            panic!("connection expected")
        };
        assert_eq!(checked.state, State::Connected);
        let after: Value = serde_json::from_slice(&stored(&node, row.id).await).unwrap();
        for field in ["auth_revision", "grant_revision", "secrets"] {
            assert_eq!(before[field], after[field], "{remote}/{field}");
        }
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 4);
            for (header, _) in requests.iter() {
                let header = header.to_ascii_lowercase();
                assert!(header.contains(&format!("authorization: bearer {SECOND}")));
                assert!(header.contains(&format!("x-client-key: {FIRST}")));
                assert!(!header.contains("x-other-key:"));
                assert!(!header.contains("x-optional:"));
            }
        }
        for value in [&row, checked] {
            let public = serde_json::to_string(value).unwrap();
            assert!(!public.contains(FIRST) && !public.contains(SECOND));
        }
        let resource = server::Server::build(Some("{}".into()), None, headers.clone()).await;
        node.set_connector_fixture(ORIGIN, &resource.origin);
        let output = client
            .execute(
                client
                    .prepare(request("/ok", row.id))
                    .with_plugin(context(row.package.clone(), worktree)),
            )
            .await
            .unwrap();
        assert!(matches!(&output, Output::PluginHttp(response) if response.status == 200));
        let public = serde_json::to_string(&output).unwrap();
        assert!(!public.contains(FIRST) && !public.contains(SECOND));
        {
            let requests = resource.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            let header = requests[0].0.to_ascii_lowercase();
            assert!(header.contains(&format!("authorization: bearer {SECOND}")));
            assert!(header.contains(&format!("x-client-key: {FIRST}")));
            assert!(!header.contains("x-other-key:") && !header.contains("x-optional:"));
        }
        for name in ["Authorization", "X-Client-Key"] {
            let mut override_header = request("/ok", row.id);
            if let Command::RequestPluginHttp(request) = &mut override_header {
                request.headers.insert(name.into(), "override".into());
            }
            let error = client
                .execute(
                    client
                        .prepare(override_header)
                        .with_plugin(context(row.package.clone(), worktree)),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert_eq!(resource.requests.lock().unwrap().len(), 1);
        }
        for secret in [FIRST, SECOND] {
            let leaked = server::Server::build(
                Some(json!({"echo":secret}).to_string()),
                None,
                headers.clone(),
            )
            .await;
            node.set_connector_fixture(ORIGIN, &leaked.origin);
            let error = client
                .execute(
                    client
                        .prepare(request("/ok", row.id))
                        .with_plugin(context(row.package.clone(), worktree)),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::PermissionDenied);
            let public = serde_json::to_string(&error).unwrap();
            assert!(!public.contains(FIRST) && !public.contains(SECOND));
            assert_eq!(leaked.requests.lock().unwrap().len(), 1);
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
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
        let before: Value = serde_json::from_slice(&stored(&node, row.id).await).unwrap();
        server.status.store(401, Ordering::SeqCst);
        let request = client.prepare(Command::ProbeConnection { connection: row.id });
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::Expired
        );
        let after: Value = serde_json::from_slice(&stored(&node, row.id).await).unwrap();
        assert_eq!(after["state"], "expired");
        for field in ["fields", "secrets", "auth_revision", "revision"] {
            assert_eq!(after[field], before[field], "{remote}/{field}");
        }
        let bytes = stored(&node, row.id).await;
        server.status.store(503, Ordering::SeqCst);
        assert_eq!(
            client
                .execute(client.prepare(Command::ProbeConnection { connection: row.id }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        assert_eq!(stored(&node, row.id).await, bytes);
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
        let before = stored(&node, row.id).await;
        node.set_connector_fixture(ORIGIN, &held.origin);
        let error = client
            .execute(client.prepare(Command::ProbeConnection { connection: row.id }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(held.held.is_cancelled());
        assert_eq!(stored(&node, row.id).await, before);
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
        let (database_handle, database) =
            crate::plugins::database_fixture::open(&(profile.join("storage/node.turso")), false)
                .await;
        let mut failed: Value = serde_json::from_slice(&before).unwrap();
        failed["state"] = json!("failed");
        database
            .execute(
                "UPDATE plugin_connections SET body=?1 WHERE id=?2",
                turso::params![serde_json::to_vec(&failed).unwrap(), row.id.to_string()],
            )
            .await
            .unwrap();
        crate::plugins::database_fixture::close(database_handle, database).await;
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
