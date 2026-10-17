use super::*;
use plugin::mcp::{Configuration, Server, State};
use serde_json::json;

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
        let Server::Http { url, headers } = configuration.servers.get_mut("service").unwrap()
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
        let db = rusqlite::Connection::open_with_flags(
            node.profile().join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let (body, result): (Vec<u8>, String) = db
            .query_row(
                "SELECT body,result FROM requests WHERE id=?1",
                [request.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(!String::from_utf8_lossy(&body).contains(secret));
        assert!(!result.contains(secret));
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM requests WHERE id=?1",
                [read_request.id.to_string()],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        let stored: Vec<u8> = db
            .query_row(
                "SELECT body FROM plugin_settings WHERE plugin='example' AND revision=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let stored: serde_json::Value = serde_json::from_slice(&stored).unwrap();
        assert_eq!(
            stored["mcp"]["mcpServers"]["service"]["headers"]["Authorization"],
            secret
        );
        let snapshot = execute(&client, Command::Snapshot).await;
        assert!(!serde_json::to_string(&snapshot).unwrap().contains(secret));
        drop(db);
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
async fn loads_bundled_documents() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _) = fixture(remote).await;
        for name in ["context7", "github"] {
            let installed = info(execute(&client, Command::ReadPlugin { name: name.into() }).await);
            let (field, header, prefix) = if name == "context7" {
                ("api_key", "Context7-API-Key", None)
            } else {
                ("token", "Authorization", Some("Bearer ".to_owned()))
            };
            let schema = installed.settings.as_ref().unwrap();
            assert_eq!(schema.schema, plugin::settings::SCHEMA);
            assert!(!schema.additional_properties);
            assert!(schema.required.is_empty());
            assert_eq!(schema.properties.len(), 1);
            let token = &schema.properties[field];
            assert_eq!(token.kind, plugin::settings::Kind::String);
            assert_eq!(token.min_length, Some(1));
            assert_eq!(token.max_length, Some(4096));
            assert!(token.default.is_none());
            assert_eq!(
                token.secret,
                Some(plugin::settings::Binding {
                    server: Some(name.into()),
                    env: None,
                    header: Some(header.into()),
                    origin: None,
                    prefix,
                    application: None,
                    oauth: None,
                    unsupported: false,
                })
            );
            let Output::PluginSettings(settings) = execute(
                &client,
                Command::ReadPluginSettings {
                    package: installed.summary.reference(),
                },
            )
            .await
            else {
                panic!("plugin settings expected")
            };
            assert_eq!(settings.package, installed.summary.reference());
            assert!(settings.values.is_empty());
            assert!(settings.configured.is_empty());
            assert_eq!(settings.keepable, vec![field.to_owned()]);
            // Optional credentials do not block the unconfigured form.
            assert!(settings.ready);
            let state = read(&client, &installed.summary.reference()).await;
            assert_eq!(state.configuration.schema, plugin::mcp::SCHEMA);
            assert_eq!(state.configuration.servers.len(), 1);
            if name == "github" {
                let Server::Http { url, headers } = &state.configuration.servers["github"] else {
                    panic!("GitHub HTTP configuration expected")
                };
                assert_eq!(url, "https://api.githubcopilot.com/mcp/");
                assert_eq!(headers.get("Authorization").map(String::as_str), Some(""));
                assert_eq!(
                    headers.get("X-MCP-Toolsets").map(String::as_str),
                    Some("repos,issues,pull_requests,actions,users")
                );
            }
            if name == "context7" {
                let Server::Http { url, headers } = &state.configuration.servers[name] else {
                    panic!("Context7 HTTP configuration expected")
                };
                assert_eq!(url, "https://mcp.context7.com/mcp");
                assert_eq!(headers.get(header).map(String::as_str), Some(""));
                let secret = "fixture-private-context7-key";
                let request = client.prepare(Command::SavePluginSettings {
                    package: installed.summary.reference(),
                    values: Default::default(),
                    secrets: std::collections::BTreeMap::from([(
                        field.into(),
                        plugin::settings::SecretUpdate::Replace(sailry_protocol::Secret::new(
                            secret.into(),
                        )),
                    )]),
                });
                assert!(!format!("{request:?}").contains(secret));
                let Output::PluginSettings(saved) = client.execute(request).await.unwrap() else {
                    panic!("saved plugin settings expected")
                };
                assert!(saved.values.is_empty());
                assert_eq!(saved.configured, vec![field.to_owned()]);
                assert!(saved.ready);
                assert!(!serde_json::to_string(&saved).unwrap().contains(secret));
                let private = read(&client, &saved.package).await;
                assert!(!serde_json::to_string(&private).unwrap().contains(secret));
                let cleared = execute(
                    &client,
                    Command::SavePluginSettings {
                        package: saved.package,
                        values: Default::default(),
                        secrets: std::collections::BTreeMap::from([(
                            field.into(),
                            plugin::settings::SecretUpdate::Clear,
                        )]),
                    },
                )
                .await;
                let Output::PluginSettings(cleared) = cleared else {
                    panic!("cleared plugin settings expected")
                };
                assert!(cleared.configured.is_empty());
                assert!(cleared.ready);
            }
            let encoded = serde_json::to_value(&state.configuration).unwrap();
            assert!(encoded.get("mcpServers").is_some());
            assert!(serde_json::from_value::<Configuration>(encoded).is_ok());
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
