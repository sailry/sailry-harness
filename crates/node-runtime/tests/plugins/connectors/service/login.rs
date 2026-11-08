use super::*;

fn declared() -> Value {
    let mut value = declaration();
    value["auth"].as_array_mut().unwrap().push(json!({
        "kind":"oauth2",
        "flow":"device_code",
        "authorize":format!("{ORIGIN}/authorize"),
        "device":format!("{ORIGIN}/device"),
        "token":format!("{ORIGIN}/token"),
        "client":{"bundled":{"client_id":"fixture-client"}},
    }));
    value
}

async fn begin(
    client: &Client,
    package: plugin::Reference,
    previous: Option<&connector::Connection>,
) -> connector::Attempt {
    let Output::ConnectorLogin(attempt) = execute(
        client,
        Command::StartConnectorLogin {
            package,
            connector: "service".into(),
            connection: previous.map(|row| row.id),
            expected_revision: previous.map(|row| row.revision),
            method: Kind::Oauth2,
            input: Input {
                secrets: previous
                    .map(|_| {
                        BTreeMap::from([(
                            "token".into(),
                            Secret::new("unconfirmed-private-token".into()),
                        )])
                    })
                    .unwrap_or_default(),
                ..Default::default()
            },
        },
    )
    .await
    else {
        panic!("connector login expected")
    };
    attempt
}

async fn expires(client: &Client, attempt: &connector::Attempt) {
    let mut updates = client.subscribe_connector_login(attempt.id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut prompted = false;
        loop {
            let Update::ConnectorLogin(update) = updates.next().await.unwrap() else {
                panic!("connector update expected")
            };
            match update.state {
                LoginState::Device { .. } => {
                    prompted = true;
                    let Output::Connections(rows) = execute(client, Command::ListConnections).await
                    else {
                        panic!("connections expected")
                    };
                    let row = rows
                        .iter()
                        .find(|row| row.id == attempt.connection)
                        .unwrap();
                    assert_eq!(row.state, State::Connecting);
                }
                LoginState::Failed(error) => {
                    assert!(prompted);
                    assert_eq!(error.code, ErrorCode::Expired);
                    break;
                }
                state => assert!(state.active(), "unexpected login state: {state:?}"),
            }
        }
    })
    .await
    .expect("pending login must expire before its next device poll");
}

#[tokio::test]
async fn restores_expired_attempts_through_remote_controller() {
    let server = server::Server::respond(
        Some(
            json!({
                "verification_uri":format!("{ORIGIN}/verify"),
                "user_code":"ABCD",
                "device_code":"private-device-code",
                "expires_in":2,
                "interval":60,
            })
            .to_string(),
        ),
        None,
    )
    .await;
    let (directory, node, controller, client, worktree) = support::empty().await;
    node.set_connector_fixture(ORIGIN, &server.origin);
    configure(&directory.path().join("source/package"), declared());
    let installed = info(execute(&client, install(worktree, 0)).await);
    assert!(installed.issues.is_empty(), "{:?}", installed.issues);
    let package = installed.summary.reference();
    let first = begin(&client, package.clone(), None).await;
    expires(&client, &first).await;
    let Output::Connections(rows) = execute(&client, Command::ListConnections).await else {
        panic!("connections expected")
    };
    let first_row = rows.iter().find(|row| row.id == first.connection).unwrap();
    assert_eq!(first_row.state, State::NotConnected);
    assert!(first_row.configured.is_empty());
    let previous = connected(
        &client,
        package.clone(),
        Some(first_row.id),
        Some(first_row.revision),
        secret(),
    )
    .await;
    let mut connections = client.subscribe_connections().await.unwrap();
    let _ = connections.next().await.unwrap();
    let replacement = begin(&client, package.clone(), Some(&previous)).await;
    expires(&client, &replacement).await;
    let restored = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Update::ConnectionsChanged(update) = connections.next().await.unwrap() else {
                panic!("connection snapshot expected")
            };
            if let Some(row) = update
                .connections
                .into_iter()
                .find(|row| row.id == previous.id && row.state == State::Connected)
            {
                break row;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(restored.method, previous.method);
    assert_eq!(restored.fields, previous.fields);
    assert_eq!(restored.configured, previous.configured);
    assert_eq!(restored.identity, previous.identity);
    assert!(restored.revision > previous.revision);
    let result = client
        .execute(
            client
                .prepare(request("/ok", previous.id))
                .with_plugin(context(package, worktree)),
        )
        .await
        .unwrap();
    assert!(matches!(result, Output::PluginHttp(response) if response.status == 200));
    let requests = server.requests.lock().unwrap().clone();
    let headers = &requests.last().unwrap().0;
    assert!(headers.contains(&format!("authorization: Bearer {TOKEN}")));
    assert!(!headers.contains("unconfirmed-private-token"));
    assert_eq!(
        client
            .execute(client.prepare(Command::SubmitConnectorInput {
                attempt: replacement.id,
                input: Secret::new("late-callback".into()),
            }))
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    drop(connections);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
