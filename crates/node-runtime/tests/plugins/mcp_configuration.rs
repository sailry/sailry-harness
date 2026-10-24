use super::*;
use plugin::mcp::{Configuration, Server, State};
use serde_json::json;
use std::collections::BTreeMap;

async fn read(client: &Client, package: &plugin::Reference) -> State {
    let Output::PluginMcp(state) = execute(
        client,
        Command::ReadPluginMcp {
            package: package.clone(),
        },
    )
    .await
    else {
        panic!("MCP configuration expected")
    };
    state
}

#[tokio::test]
async fn recovers_standard_configuration() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let source = directory.path().join("source/package/mcp.json");
        fs::write(&source, json!({"$schema":plugin::mcp::SCHEMA,"mcpServers":{
            "service":{"type":"streamable-http","url":"https://example.com/mcp","headers":{"Authorization":""}}
        }}).to_string()).unwrap();
        let installed = info(execute(&client, install(worktree, 0)).await);
        let initial = read(&client, &installed.summary.reference()).await;
        let secret = "Bearer inline-private-${PLUGIN_ROOT}";
        let mut configuration = initial.configuration.clone();
        let Server::Http { url, headers, .. } = configuration.servers.get_mut("service").unwrap()
        else {
            unreachable!()
        };
        *url = "https://configured.example.com/mcp".into();
        headers.insert("Authorization".into(), secret.into());
        headers.insert("X-Context7-API-Key".into(), "literal-${API_KEY}".into());
        let request = client.prepare(Command::SavePluginMcp {
            package: initial.package.clone(),
            configuration: configuration.clone(),
        });
        assert!(!format!("{request:?}").contains(secret));
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let first = info(admission.completion.await.unwrap().unwrap());
        assert_eq!(first.summary.settings_revision, 1);
        assert!(!serde_json::to_string(&first).unwrap().contains(secret));
        let read_request = client.prepare(Command::ReadPluginMcp {
            package: first.summary.reference(),
        });
        let admission = client.dispatch(read_request.clone()).await.unwrap();
        assert!(!admission.receipt.durable);
        let Output::PluginMcp(saved) = admission.completion.await.unwrap().unwrap() else {
            panic!("MCP configuration expected")
        };
        assert_eq!(saved.configuration, configuration);
        assert_eq!(fs::read_to_string(&source).unwrap(), json!({"$schema":plugin::mcp::SCHEMA,"mcpServers":{
            "service":{"type":"streamable-http","url":"https://example.com/mcp","headers":{"Authorization":""}}
        }}).to_string());
        let context = plugin::Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: saved.package.clone(),
            worktree: Some(worktree),
            session: None,
        };
        for command in [
            Command::ReadPluginMcp {
                package: saved.package.clone(),
            },
            Command::SavePluginMcp {
                package: saved.package.clone(),
                configuration: configuration.clone(),
            },
            Command::SetPluginMcpAllowed {
                package: saved.package.clone(),
                server: "service".into(),
                allowed: false,
            },
            Command::CheckPluginMcp {
                package: saved.package.clone(),
                server: "service".into(),
            },
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command).with_plugin(context.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let mut invalid = configuration.clone();
        let server = invalid.servers.remove("service").unwrap();
        invalid.servers.insert("other".into(), server);
        assert_eq!(
            client
                .execute(client.prepare(Command::SavePluginMcp {
                    package: saved.package.clone(),
                    configuration: invalid
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let mut replacement = configuration.clone();
        let Server::Http { headers, .. } = replacement.servers.get_mut("service").unwrap() else {
            unreachable!()
        };
        headers.insert("Authorization".into(), "Bearer next-private-token".into());
        let latest = info(
            execute(
                &client,
                Command::SavePluginMcp {
                    package: saved.package.clone(),
                    configuration: replacement.clone(),
                },
            )
            .await,
        );
        assert_eq!(
            client.execute(request.clone()).await.unwrap(),
            Output::Plugin(first.clone())
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::SavePluginMcp {
                    package: saved.package,
                    configuration: configuration.clone()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );

        let (body, result): (Vec<u8>, String) = {
            let sql = ("SELECT body,result FROM requests WHERE id=?1").to_owned();
            let parameters = turso::params![request.id.to_string()];
            node.with_storage(move |db| {
                Box::pin(async move {
                    let row = db
                        .prepare(sql)
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?
                        .query_row(parameters)
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?;
                    let value: turso::Result<_> =
                        (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                    value.map_err(crate::plugins::database_fixture::fault)
                })
            })
            .await
        }
        .unwrap();
        assert!(!String::from_utf8_lossy(&body).contains(secret));
        assert!(!result.contains(secret));
        assert_eq!(
            {
                let sql = ("SELECT count(*) FROM requests WHERE id=?1").to_owned();
                let parameters = turso::params![read_request.id.to_string()];
                node.with_storage(move |db| {
                    Box::pin(async move {
                        let row = db
                            .prepare(sql)
                            .await
                            .map_err(crate::plugins::database_fixture::fault)?
                            .query_row(parameters)
                            .await
                            .map_err(crate::plugins::database_fixture::fault)?;
                        let value: turso::Result<_> = (|row: &turso::Row| row.get::<i64>(0))(&row);
                        value.map_err(crate::plugins::database_fixture::fault)
                    })
                })
                .await
            }
            .unwrap(),
            0
        );
        let stored: Vec<u8> = {
            let sql = ("SELECT body FROM plugin_settings WHERE plugin='example' AND revision=1")
                .to_owned();
            let parameters = ();
            node.with_storage(move |db| {
                Box::pin(async move {
                    let row = db
                        .prepare(sql)
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?
                        .query_row(parameters)
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?;
                    let value: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                    value.map_err(crate::plugins::database_fixture::fault)
                })
            })
            .await
        }
        .unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&stored).unwrap();
        assert_eq!(
            stored["mcp"]["mcpServers"]["service"]["headers"]["Authorization"],
            secret
        );
        let snapshot = execute(&client, Command::Snapshot).await;
        assert!(!serde_json::to_string(&snapshot).unwrap().contains(secret));

        node.shutdown().await.unwrap();
        let reopened = Node::start_empty_with_plugin_catalog(
            directory.path().join("node"),
            "http://127.0.0.1:9/",
        )
        .await
        .unwrap();
        let reconnected = Client::new(if remote {
            controller.handle().remote(reopened.link().address())
        } else {
            reopened.local()
        });
        assert_eq!(
            read(&reconnected, &latest.summary.reference())
                .await
                .configuration,
            replacement
        );
        assert_eq!(
            reconnected.execute(request).await.unwrap(),
            Output::Plugin(first)
        );
        reopened.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn edits_unbound_siblings() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let root = directory.path().join("source/package");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("plugin.json")).unwrap()).unwrap();
        manifest["extensions"] = json!({"dev.sailry.platform":{
            "api_version":"v1", "actions":[], "settings_schema":"dev.sailry.platform/settings.json"
        }});
        fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
        fs::write(root.join("dev.sailry.platform/settings.json"), json!({
            "$schema":plugin::settings::SCHEMA, "type":"object", "additionalProperties":false,
            "properties":{
                "api_key":{"type":"string", "x-sailry-secret":{"server":"private_http", "header":"Authorization"}},
                "token":{"type":"string", "x-sailry-secret":{"server":"private_stdio", "env":"TOKEN"}}
            }
        }).to_string()).unwrap();
        fs::write(root.join("mcp.json"), json!({
            "$schema":plugin::mcp::SCHEMA,"mcpServers":{
                "private_http":{"type":"streamable-http", "url":"https://private.example.com/mcp", "headers":{"Authorization":""}},
                "private_stdio":{"type":"stdio", "command":"never-run-during-configuration", "env":{"TOKEN":""}},
                "public":{"type":"streamable-http", "url":"https://public.example.com/mcp"}
            }
        }).to_string()).unwrap();
        let installed = info(execute(&client, install(worktree, 0)).await);
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
        let Output::PluginSettings(settings) = execute(
            &client,
            Command::SavePluginSettings {
                package: installed.summary.reference(),
                values: BTreeMap::new(),
                secrets: BTreeMap::from([
                    (
                        "api_key".into(),
                        plugin::settings::SecretUpdate::Replace(Secret::new(
                            "private-api-key".into(),
                        )),
                    ),
                    (
                        "token".into(),
                        plugin::settings::SecretUpdate::Replace(Secret::new(
                            "private-stdio-token".into(),
                        )),
                    ),
                ]),
            },
        )
        .await
        else {
            panic!("plugin settings expected")
        };
        let original = read(&client, &settings.package).await.configuration;
        let mut configuration = original.clone();
        let Server::Http { url, .. } = configuration.servers.get_mut("public").unwrap() else {
            unreachable!()
        };
        *url = "https://configured.example.com/mcp".into();
        let request = client.prepare(Command::SavePluginMcp {
            package: settings.package,
            configuration: configuration.clone(),
        });
        let first = info(client.execute(request.clone()).await.unwrap());
        let Server::Http { headers, .. } = configuration.servers.get_mut("public").unwrap() else {
            unreachable!()
        };
        headers.insert("X-Context".into(), "updated".into());
        let latest = info(
            execute(
                &client,
                Command::SavePluginMcp {
                    package: first.summary.reference(),
                    configuration: configuration.clone(),
                },
            )
            .await,
        );
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::Plugin(first.clone())
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::SavePluginMcp {
                    package: first.summary.reference(),
                    configuration: configuration.clone(),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        for name in ["private_http", "private_stdio"] {
            assert_eq!(configuration.servers[name], original.servers[name]);
        }
        for (path, replacement) in [
            (
                "/mcpServers/private_http/url",
                json!("https://changed.example.com/mcp"),
            ),
            (
                "/mcpServers/private_http/headers/Authorization",
                json!("Bearer forbidden"),
            ),
            (
                "/mcpServers/private_stdio/command",
                json!("never-run-a-different-command"),
            ),
            ("/mcpServers/private_stdio/env/TOKEN", json!("forbidden")),
        ] {
            let mut invalid = serde_json::to_value(&configuration).unwrap();
            *invalid.pointer_mut(path).unwrap() = replacement;
            let invalid = serde_json::from_value(invalid).unwrap();
            assert_eq!(
                client
                    .execute(client.prepare(Command::SavePluginMcp {
                        package: latest.summary.reference(),
                        configuration: invalid,
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
            assert_eq!(
                read(&client, &latest.summary.reference())
                    .await
                    .configuration,
                configuration
            );
        }
        assert_eq!(
            info(
                execute(
                    &client,
                    Command::ReadPlugin {
                        name: "example".into()
                    }
                )
                .await
            ),
            latest
        );
        let Output::PluginSettings(kept) = execute(
            &client,
            Command::SavePluginSettings {
                package: latest.summary.reference(),
                values: BTreeMap::new(),
                secrets: BTreeMap::from([
                    ("api_key".into(), plugin::settings::SecretUpdate::Keep),
                    ("token".into(), plugin::settings::SecretUpdate::Keep),
                ]),
            },
        )
        .await
        else {
            panic!("plugin settings expected")
        };
        assert!(kept.ready);
        assert_eq!(kept.configured, ["api_key", "token"]);
        assert_eq!(kept.keepable, ["api_key", "token"]);
        assert_eq!(
            read(&client, &kept.package).await.configuration,
            configuration
        );
        let state = serde_json::to_string(&read(&client, &kept.package).await).unwrap();
        assert!(!state.contains("private-api-key"));
        assert!(!state.contains("private-stdio-token"));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn loads_bundled_documents() {
    use plugin::connector::{self, Auth, Kind, LoginState, State as ConnectionState};

    for remote in [false, true] {
        let (_directory, node, controller, client, _) = fixture(remote).await;
        for name in ["context7", "github"] {
            let installed = info(execute(&client, Command::ReadPlugin { name: name.into() }).await);
            let (field, header, prefix) = if name == "context7" {
                ("api_key", "Context7-API-Key", "")
            } else {
                ("token", "Authorization", "Bearer ")
            };
            assert!(installed.settings.is_none());
            let declaration = &installed.extension.as_ref().unwrap().connectors[0];
            assert_eq!(declaration.id, name);
            assert_eq!(declaration.accounts, connector::Accounts::Single);
            assert_eq!(
                declaration.auth[0],
                Auth::Secret {
                    fields: vec![field.into()]
                }
            );
            if name == "github" {
                assert_eq!(declaration.auth.len(), 2);
                assert!(
                    matches!(&declaration.auth[1], Auth::LocalCli {command, ..} if command == "gh")
                );
            } else {
                assert_eq!(declaration.auth.len(), 1);
            }
            assert_eq!(declaration.fields.len(), 1);
            let token = &declaration.fields[0];
            assert_eq!(token.name, field);
            assert!(token.secret);
            assert_eq!(token.optional, name == "context7");
            assert!(token.default.is_none());
            assert_eq!(
                declaration.inject,
                vec![connector::Injection {
                    server: Some(name.into()),
                    env: None,
                    header: Some(header.into()),
                    origin: None,
                    prefix: prefix.into(),
                    field: None,
                }]
            );
            assert!(declaration.identity.is_none());
            assert!(declaration.health.is_none());
            let Output::ConnectorTypes(types) = execute(&client, Command::ListConnectorTypes).await
            else {
                panic!("connector types expected")
            };
            let managed = types
                .iter()
                .find(|item| {
                    item.package == installed.summary.reference() && item.declaration.id == name
                })
                .unwrap();
            assert!(managed.enabled);
            assert_eq!(&managed.declaration, declaration);
            assert_eq!(
                client
                    .execute(client.prepare(Command::ReadPluginSettings {
                        package: installed.summary.reference(),
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotConfigured
            );
            let state = read(&client, &installed.summary.reference()).await;
            assert_eq!(state.configuration.schema, plugin::mcp::SCHEMA);
            assert_eq!(state.configuration.servers.len(), 1);
            let Server::Http { url, headers, .. } = &state.configuration.servers[name] else {
                panic!("bundled HTTP configuration expected")
            };
            assert!(headers.keys().all(|key| !key.eq_ignore_ascii_case(header)));
            if name == "github" {
                assert_eq!(url, "https://api.githubcopilot.com/mcp/");
                assert_eq!(declaration.origins, ["https://api.githubcopilot.com"]);
                assert_eq!(
                    headers.get("X-MCP-Toolsets").map(String::as_str),
                    Some("repos,issues,pull_requests,actions,users")
                );
            } else {
                assert_eq!(url, "https://mcp.context7.com/mcp");
                assert_eq!(declaration.origins, ["https://mcp.context7.com"]);
            }
            let encoded = serde_json::to_value(&state.configuration).unwrap();
            assert!(encoded.get("mcpServers").is_some());
            assert_eq!(
                serde_json::from_value::<Configuration>(encoded).unwrap(),
                state.configuration
            );

            let secret = format!("fixture-private-{name}-key");
            let request = client.prepare(Command::StartConnectorLogin {
                package: installed.summary.reference(),
                connector: name.into(),
                connection: None,
                expected_revision: None,
                method: Kind::Secret,
                input: connector::Input {
                    secrets: BTreeMap::from([(field.into(), Secret::new(secret.clone()))]),
                    ..Default::default()
                },
            });
            assert!(!format!("{request:?}").contains(&secret));
            let Output::ConnectorLogin(attempt) = client.execute(request.clone()).await.unwrap()
            else {
                panic!("connector login expected")
            };
            assert_eq!(
                client.execute(request).await.unwrap(),
                Output::ConnectorLogin(attempt.clone())
            );
            let mut changes = client.subscribe_connector_login(attempt.id).await.unwrap();
            let connected = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let Update::ConnectorLogin(update) = changes.next().await.unwrap() else {
                        panic!("connector update expected")
                    };
                    assert!(!serde_json::to_string(&update).unwrap().contains(&secret));
                    match update.state {
                        LoginState::Connected(connection) => break connection,
                        pending => assert!(pending.active(), "unexpected state: {pending:?}"),
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(connected.package, installed.summary.reference());
            assert_eq!(connected.connector, name);
            assert_eq!(connected.method, Kind::Secret);
            assert_eq!(connected.state, ConnectionState::Connected);
            assert_eq!(connected.configured, [field]);
            assert_eq!(read(&client, &state.package).await, state);
            assert!(!serde_json::to_string(&connected).unwrap().contains(&secret));
            assert!(!serde_json::to_string(&state).unwrap().contains(&secret));
            assert!(
                !serde_json::to_string(&execute(&client, Command::Snapshot).await)
                    .unwrap()
                    .contains(&secret)
            );
            let Output::Connection(cleared) = execute(
                &client,
                Command::DisconnectConnection {
                    connection: connected.id,
                    expected_revision: connected.revision,
                },
            )
            .await
            else {
                panic!("disconnected connection expected")
            };
            assert_eq!(cleared.id, connected.id);
            assert_eq!(cleared.package, connected.package);
            assert_eq!(cleared.connector, name);
            assert_eq!(cleared.state, ConnectionState::NotConnected);
            assert!(cleared.configured.is_empty());
            assert!(cleared.revision > connected.revision);
            assert_eq!(read(&client, &state.package).await, state);
            assert!(!serde_json::to_string(&cleared).unwrap().contains(&secret));
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
