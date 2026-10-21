use super::*;
use sailry_protocol::plugin::connector::State;

#[tokio::test]
async fn restores_slack_clients_and_mcp_credentials() {
    for remote in [false, true] {
        let peer = input::Server::url("https://example.invalid/continue", false).await;
        let issuer =
            issuer::Server::confidential(&peer.endpoint, true, issuer::Authentication::Post).await;
        let model = Server::turn_tools(vec![(
            mcp_peer::package_alias("slack", "slack", "read"),
            json!({}),
        )])
        .await;
        let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
        fixture
            .node
            .set_connector_fixture("https://mcp.slack.com", &issuer.endpoint);
        let Output::Plugin(mut info) = execute(
            &fixture.client,
            Command::InstallBundledPlugin {
                name: "slack".into(),
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        assert!(info.issues.is_empty(), "{:?}", info.issues);
        if !info.summary.enabled {
            let Output::Plugin(enabled) = execute(
                &fixture.client,
                Command::SetPluginEnabled {
                    name: "slack".into(),
                    expected_revision: info.summary.revision,
                    enabled: true,
                },
            )
            .await
            else {
                panic!("plugin expected")
            };
            info = enabled;
        }
        let Output::ConnectorLogin(attempt) = execute(
            &fixture.client,
            Command::StartConnectorLogin {
                package: info.summary.reference(),
                connector: "slack".into(),
                connection: None,
                expected_revision: None,
                method: Kind::McpOauth,
                input: Input {
                    redirect: Some("http://127.0.0.1:43191/auth/callback".into()),
                    fields: BTreeMap::from([("client_id".into(), "fixture-client".into())]),
                    secrets: BTreeMap::from([(
                        "client_secret".into(),
                        Secret::new("client-private-marker".into()),
                    )]),
                    ..Default::default()
                },
            },
        )
        .await
        else {
            panic!("login expected")
        };
        let LoginState::Browser { url, .. } = state(&fixture.client, &attempt, |state| {
            matches!(state, LoginState::Browser { .. })
        })
        .await
        else {
            unreachable!()
        };
        let connected = complete(&fixture.client, &attempt, &url).await;
        lifecycle::call(&fixture).await;
        assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
        let profile = fixture.node.profile().to_owned();
        let id = fixture.node.id();
        fixture.node.shutdown().await.unwrap();
        fixture.controller.handle().disconnect(id).await;
        let path = profile.join("storage/node.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        let bytes: Vec<u8> = db
            .query_row(
                "SELECT body FROM plugin_connections WHERE id=?1",
                [connected.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let stored: Value = serde_json::from_slice(&bytes).unwrap();
        let saved = json!({"schema":null,"values":{"client_id":"fixture-client"},"slots":{"client_secret":"client-private-marker"},
            "authorizations":{"slack":{"id":CredentialId::new(),"endpoint":"https://mcp.slack.com/mcp","revision":3,"credentials":stored["grant"]["access"]}}});
        let original = serde_json::to_vec_pretty(&saved).unwrap();
        info.summary.version = Some("0.0.1".into());
        info.summary.settings_revision = 1;
        info.origin = Some(plugin::Origin::Bundled);
        info.extension.as_mut().unwrap().connectors.clear();
        db.execute(
            "INSERT INTO plugin_settings(plugin,revision,body) VALUES('slack',1,?1)",
            [&original],
        )
        .unwrap();
        db.execute(
            "UPDATE plugins SET settings_revision=1,body=?1 WHERE name='slack'",
            [serde_json::to_vec(&info).unwrap()],
        )
        .unwrap();
        db.execute("DELETE FROM plugin_connections WHERE plugin='slack'", [])
            .unwrap();
        db.close().unwrap();
        fixture.node = Node::start_empty(&profile).await.unwrap();
        fixture
            .node
            .set_connector_fixture("https://mcp.slack.com", &issuer.endpoint);
        fixture.client = Client::new(if remote {
            fixture
                .controller
                .handle()
                .remote(fixture.node.link().address())
        } else {
            fixture.node.local()
        });
        execute(
            &fixture.client,
            Command::InstallBundledPlugin {
                name: "slack".into(),
                expected_revision: info.summary.revision,
            },
        )
        .await;
        let Output::Connections(rows) = execute(&fixture.client, Command::ListConnections).await
        else {
            panic!("connections expected")
        };
        let row = rows.iter().find(|row| row.package.name == "slack").unwrap();
        assert_eq!(row.state, State::Connected);
        assert_eq!(row.method, Kind::McpOauth);
        assert_eq!(row.fields["client_id"], "fixture-client");
        assert!(row.configured.contains(&"client_secret".into()));
        let page = lifecycle::call(&fixture).await;
        assert_eq!(peer.calls.load(Ordering::SeqCst), 2);
        issuer.expire();
        lifecycle::recover(&fixture, &peer, 3).await;
        assert_eq!(issuer.registrations.load(Ordering::SeqCst), 0);
        assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
        assert_eq!(issuer.refreshes.load(Ordering::SeqCst), 1);
        let db = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let retained: Vec<u8> = db
            .query_row(
                "SELECT body FROM plugin_settings WHERE plugin='slack' AND revision=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained, original);
        db.close().unwrap();
        let public = serde_json::to_string(&(rows, page)).unwrap();
        for secret in ["client-private-marker", "access-initial", "refresh-initial"] {
            assert!(!public.contains(secret));
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
