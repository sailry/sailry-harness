//! Registration exercises the real Node owner through local and remote Client paths.
use super::*;
use plugin::{
    oauth::{Attempt, State},
    settings::SecretUpdate,
};
use serde_json::json;
use std::collections::BTreeMap;
#[path = "registration/server.rs"]
mod server;

fn configure(root: &Path) {
    let manifest = root.join("plugin.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["extensions"] = json!({"dev.sailry.platform":{
        "api_version":"v1","scope":"host","actions":[],
        "settings_schema":"dev.sailry.platform/settings.json",
        "desktop":{"resources":["dev.sailry.platform/desktop.js"]},
        "settings_page":{"navigation":{"label":"Application"},"entry":"dev.sailry.platform/desktop.js"},
        "registration":{"provider":"feishu","client_id":"app_id","client_secret":"app_secret","tenant_scopes":["docx:document:readonly","docs:document.content:read","docs:document.comment:read"]}
    }});
    fs::write(manifest, value.to_string()).unwrap();
    fs::write(
        root.join("dev.sailry.platform/desktop.js"),
        "export default class Application {}\n",
    )
    .unwrap();
    fs::write(root.join("mcp.json"),json!({"$schema":plugin::mcp::SCHEMA,"mcpServers":{"feishu":{"type":"streamable-http","url":"https://mcp.feishu.cn/mcp","headers":{"X-Lark-MCP-TAT":""}}}}).to_string()).unwrap();
    fs::write(root.join("dev.sailry.platform/settings.json"), json!({
        "$schema":plugin::settings::SCHEMA,"type":"object","additionalProperties":false,
        "properties":{
            "app_id":{"type":"string","minLength":1,"maxLength":256},
            "app_secret":{"type":"string","x-sailry-secret":{"server":"feishu","header":"X-Lark-MCP-TAT","application":{"provider":"feishu","client_id":"app_id"}}},
            "note":{"type":"string","default":"keep draft"},
            "other":{"type":"string","x-sailry-secret":{"origin":"https://example.com","header":"Authorization"}}
        },"required":["app_id","app_secret"]
    }).to_string()).unwrap();
}

fn installed(output: Output) -> plugin::Info {
    let package = info(output);
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    assert!(package.settings.is_some());
    package
}

async fn state(client: &Client, attempt: &Attempt, ready: impl Fn(&State) -> bool) -> State {
    let mut subscription = client.subscribe_plugin_login(attempt.id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let Update::PluginLogin(update) = subscription.next().await.unwrap() else {
                panic!("application registration update expected")
            };
            let public = serde_json::to_string(&update).unwrap();
            for marker in [
                "private-device-marker",
                "private-app-secret-marker",
                "unused-private-user-marker",
            ] {
                assert!(!public.contains(marker));
            }
            if ready(&update.state) {
                return update.state;
            }
            assert!(update.state.active(), "{:?}", update.state);
        }
    })
    .await
    .expect("registration state deadline")
}

#[tokio::test]
async fn persists_credentials_without_public_secrets() {
    for remote in [false, true] {
        let issuer = server::Server::start(vec![]).await;
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        node.set_plugin_registration_fixture(&issuer.endpoint);
        configure(&directory.path().join("source/package"));
        let package = installed(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let Output::PluginSettings(initial) = execute(
            &client,
            Command::SavePluginSettings {
                package,
                values: BTreeMap::from([
                    ("app_id".into(), json!("cli_previous")),
                    ("note".into(), json!("preserve unrelated draft")),
                ]),
                secrets: BTreeMap::from([
                    (
                        "app_secret".into(),
                        SecretUpdate::Replace(Secret::new("previous-private-marker".into())),
                    ),
                    (
                        "other".into(),
                        SecretUpdate::Replace(Secret::new("other-private-marker".into())),
                    ),
                ]),
            },
        )
        .await
        else {
            panic!("settings expected")
        };
        let scope = plugin::Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Settings,
            package: initial.package.clone(),
            worktree: None,
            session: None,
        };
        let request = client
            .prepare(Command::BeginPluginRegistration {
                package: initial.package.clone(),
            })
            .with_plugin(scope.clone());
        let Output::PluginLogin(attempt) = client.execute(request.clone()).await.unwrap() else {
            panic!("attempt expected")
        };
        let State::Pending { url, .. } = state(&client, &attempt, |state| {
            matches!(state, State::Pending { .. })
        })
        .await
        else {
            unreachable!()
        };
        assert!(url.contains("createOnly=true"));
        assert_eq!(
            client.execute(request.clone()).await.unwrap(),
            Output::PluginLogin(attempt.clone())
        );
        assert_eq!(issuer.requests.lock().unwrap().iter().filter(|(_,body)|body=="action=begin&archetype=PersonalAgent&auth_method=client_secret&request_user_info=open_id").count(),1);
        assert_eq!(
            client
                .execute(client.prepare(Command::CompletePluginLogin {
                    attempt: attempt.id,
                    callback: Secret::new("forged".into())
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        issuer
            .replies
            .lock()
            .unwrap()
            .push_back(server::Server::success());
        let State::Connected(package) = state(&client, &attempt, |state| {
            matches!(state, State::Connected(_))
        })
        .await
        else {
            unreachable!()
        };
        assert_eq!(
            package.settings_revision,
            initial.package.settings_revision + 1
        );
        let Output::PluginSettings(saved) = execute(
            &client,
            Command::ReadPluginSettings {
                package: package.clone(),
            },
        )
        .await
        else {
            panic!("settings expected")
        };
        assert_eq!(saved.values["app_id"], "cli_registered");
        assert_eq!(saved.values["note"], "preserve unrelated draft");
        for (field, expected) in [
            ("app_secret", "private-app-secret-marker"),
            ("other", "other-private-marker"),
        ] {
            let Output::PluginSecret(secret) = execute(
                &client,
                Command::ReadPluginSecret {
                    package: package.clone(),
                    field: field.into(),
                },
            )
            .await
            else {
                panic!("private controller setting expected")
            };
            assert_eq!(secret.unwrap().expose(), expected);
        }
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let encoded = serde_json::to_string(&(saved, snapshot)).unwrap();
        for marker in [
            "private-device-marker",
            "private-app-secret-marker",
            "other-private-marker",
            "unused-private-user-marker",
            "single-code",
        ] {
            assert!(!encoded.contains(marker));
        }
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::PluginLogin(attempt)
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_stale_scopes_and_cancels_polling() {
    for remote in [false, true] {
        let issuer = server::Server::start(vec![]).await;
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        node.set_plugin_registration_fixture(&issuer.endpoint);
        configure(&directory.path().join("source/package"));
        let package = installed(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        for surface in [
            plugin::desktop::Surface::Workspace,
            plugin::desktop::Surface::Composer,
        ] {
            let request = client
                .prepare(Command::BeginPluginRegistration {
                    package: package.clone(),
                })
                .with_plugin(plugin::Context {
                    invocation: None,
                    turn: None,
                    surface,
                    package: package.clone(),
                    worktree: None,
                    session: None,
                });
            assert_eq!(
                client.execute(request).await.unwrap_err().code,
                ErrorCode::PermissionDenied
            );
        }
        assert!(issuer.requests.lock().unwrap().is_empty());
        let mut stale = package.clone();
        stale.settings_revision += 1;
        assert_eq!(
            client
                .execute(client.prepare(Command::BeginPluginRegistration { package: stale }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let Output::PluginLogin(attempt) = execute(
            &client,
            Command::BeginPluginRegistration {
                package: package.clone(),
            },
        )
        .await
        else {
            panic!("attempt expected")
        };
        state(&client, &attempt, |state| {
            matches!(state, State::Pending { .. })
        })
        .await;
        let other = Client::new(if remote {
            node.local()
        } else {
            controller.handle().remote(node.link().address())
        });
        assert_eq!(
            other
                .execute(other.prepare(Command::CancelPluginLogin {
                    attempt: attempt.id
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let cancel = client.prepare(Command::CancelPluginLogin {
            attempt: attempt.id,
        });
        assert_eq!(
            client.execute(cancel.clone()).await.unwrap(),
            Output::PluginLogin(attempt.clone())
        );
        state(&client, &attempt, |state| matches!(state, State::Cancelled)).await;
        let count = issuer.requests.lock().unwrap().len();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert_eq!(issuer.requests.lock().unwrap().len(), count);
        assert_eq!(
            client.execute(cancel).await.unwrap(),
            Output::PluginLogin(attempt)
        );
        let Output::PluginSettings(saved) =
            execute(&client, Command::ReadPluginSettings { package }).await
        else {
            panic!("settings expected")
        };
        assert!(saved.configured.is_empty());
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_settings_after_revision_change() {
    for remote in [false, true] {
        for change in ["unrelated", "id", "secret", "cleared", "disabled"] {
            let issuer = server::Server::start(vec![]).await;
            let (directory, node, controller, client, worktree) = fixture(remote).await;
            node.set_plugin_registration_fixture(&issuer.endpoint);
            configure(&directory.path().join("source/package"));
            let package = installed(execute(&client, install(worktree, 0)).await)
                .summary
                .reference();
            let Output::PluginSettings(initial) = execute(
                &client,
                Command::SavePluginSettings {
                    package,
                    values: BTreeMap::from([
                        ("app_id".into(), json!("cli_previous")),
                        ("note".into(), json!("retain previous note")),
                    ]),
                    secrets: BTreeMap::from([
                        (
                            "app_secret".into(),
                            SecretUpdate::Replace(Secret::new("previous-private-marker".into())),
                        ),
                        (
                            "other".into(),
                            SecretUpdate::Replace(Secret::new("other-private-marker".into())),
                        ),
                    ]),
                },
            )
            .await
            else {
                panic!("settings expected")
            };
            let Output::PluginLogin(attempt) = execute(
                &client,
                Command::BeginPluginRegistration {
                    package: initial.package.clone(),
                },
            )
            .await
            else {
                panic!("attempt expected")
            };
            state(&client, &attempt, |state| {
                matches!(state, State::Pending { .. })
            })
            .await;
            let current = if change == "disabled" {
                let installed = info(
                    execute(
                        &client,
                        Command::ReadPlugin {
                            name: initial.package.name.clone(),
                        },
                    )
                    .await,
                );
                info(
                    execute(
                        &client,
                        Command::SetPluginEnabled {
                            name: initial.package.name.clone(),
                            expected_revision: installed.summary.revision,
                            enabled: false,
                        },
                    )
                    .await,
                )
                .summary
                .reference()
            } else {
                let Output::PluginSettings(saved) = execute(
                    &client,
                    Command::SavePluginSettings {
                        package: initial.package.clone(),
                        values: BTreeMap::from([
                            (
                                "app_id".into(),
                                json!(if change == "id" {
                                    "cli_changed"
                                } else {
                                    "cli_previous"
                                }),
                            ),
                            ("note".into(), json!("newer explicit setting")),
                        ]),
                        secrets: BTreeMap::from([
                            (
                                "app_secret".into(),
                                if change == "cleared" {
                                    SecretUpdate::Clear
                                } else if change == "secret" {
                                    SecretUpdate::Replace(Secret::new(
                                        "changed-private-marker".into(),
                                    ))
                                } else {
                                    SecretUpdate::Keep
                                },
                            ),
                            (
                                "other".into(),
                                SecretUpdate::Replace(Secret::new(
                                    "new-other-private-marker".into(),
                                )),
                            ),
                        ]),
                    },
                )
                .await
                else {
                    panic!("settings expected")
                };
                saved.package
            };
            issuer
                .replies
                .lock()
                .unwrap()
                .push_back(server::Server::success());
            let result = state(&client, &attempt, |state| !state.active()).await;
            let current = match result {
                State::Connected(saved) if change == "unrelated" => {
                    assert_eq!(saved.settings_revision, current.settings_revision + 1);
                    saved
                }
                State::Failed(fault) if change != "unrelated" => {
                    assert_eq!(
                        fault.code,
                        if change == "disabled" {
                            ErrorCode::NotConfigured
                        } else {
                            ErrorCode::RevisionConflict
                        }
                    );
                    current
                }
                _ => panic!("unexpected registration result"),
            };
            let Output::PluginSettings(saved) = execute(
                &client,
                Command::ReadPluginSettings {
                    package: current.clone(),
                },
            )
            .await
            else {
                panic!("settings expected")
            };
            assert_eq!(saved.package, current);
            assert_eq!(
                saved.values["app_id"],
                match change {
                    "unrelated" => "cli_registered",
                    "id" => "cli_changed",
                    _ => "cli_previous",
                }
            );
            assert_eq!(
                saved.values["note"],
                if change == "disabled" {
                    "retain previous note"
                } else {
                    "newer explicit setting"
                }
            );
            for (field, expected) in [
                (
                    "app_secret",
                    match change {
                        "unrelated" => "private-app-secret-marker",
                        "secret" => "changed-private-marker",
                        _ => "previous-private-marker",
                    },
                ),
                (
                    "other",
                    if change == "disabled" {
                        "other-private-marker"
                    } else {
                        "new-other-private-marker"
                    },
                ),
            ] {
                let Output::PluginSecret(secret) = execute(
                    &client,
                    Command::ReadPluginSecret {
                        package: current.clone(),
                        field: field.into(),
                    },
                )
                .await
                else {
                    panic!("private setting expected")
                };
                if field == "app_secret" && change == "cleared" {
                    assert!(secret.is_none());
                } else {
                    assert_eq!(secret.unwrap().expose(), expected);
                }
            }
            assert!(
                !serde_json::to_string(&saved)
                    .unwrap()
                    .contains("private-marker")
            );
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn cancels_original_before_explicit_refresh() {
    for remote in [false, true] {
        let issuer = server::Server::start(vec![]).await;
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        node.set_plugin_registration_fixture(&issuer.endpoint);
        configure(&directory.path().join("source/package"));
        let package = installed(execute(&client, install(worktree, 0)).await)
            .summary
            .reference();
        let original = client.prepare(Command::BeginPluginRegistration {
            package: package.clone(),
        });
        let Output::PluginLogin(first) = client.execute(original.clone()).await.unwrap() else {
            panic!("attempt expected")
        };
        state(&client, &first, |state| {
            matches!(state, State::Pending { .. })
        })
        .await;
        let cancel = client.prepare(Command::CancelPluginLogin { attempt: first.id });
        assert_eq!(
            client.execute(cancel.clone()).await.unwrap(),
            Output::PluginLogin(first.clone())
        );
        let refresh = client.prepare(Command::BeginPluginRegistration {
            package: package.clone(),
        });
        let Output::PluginLogin(second) = client.execute(refresh.clone()).await.unwrap() else {
            panic!("attempt expected")
        };
        assert_ne!(first.id, second.id);
        assert_eq!(second.package, package);
        state(&client, &second, |state| {
            matches!(state, State::Pending { .. })
        })
        .await;
        assert_eq!(
            client.execute(original).await.unwrap(),
            Output::PluginLogin(first.clone())
        );
        assert_eq!(
            client.execute(cancel).await.unwrap(),
            Output::PluginLogin(first.clone())
        );
        assert_eq!(
            client.execute(refresh).await.unwrap(),
            Output::PluginLogin(second.clone())
        );
        state(&client, &first, |state| matches!(state, State::Cancelled)).await;
        let records = issuer.requests.lock().unwrap();
        for action in ["init", "begin"] {
            assert_eq!(
                records
                    .iter()
                    .filter(|(_, body)| url::form_urlencoded::parse(body.as_bytes())
                        .any(|(key, value)| key == "action" && value == action))
                    .count(),
                2
            );
        }
        drop(records);
        execute(&client, Command::CancelPluginLogin { attempt: second.id }).await;
        state(&client, &second, |state| matches!(state, State::Cancelled)).await;
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
