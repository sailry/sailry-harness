use super::*;

async fn settings(client: &Client) -> code::Settings {
    let Output::CodeSettings(settings) = client
        .execute(client.prepare(Command::ReadCodeSettings))
        .await
        .unwrap()
    else {
        panic!("settings expected")
    };
    settings
}

async fn save(
    client: &Client,
    value: code::Settings,
) -> (sailry_protocol::Request, code::Settings) {
    let request = client.prepare(Command::SaveCodeSettings {
        expected_revision: value.revision,
        settings: value,
    });
    let Output::CodeSettings(saved) = client.execute(request.clone()).await.unwrap() else {
        panic!("settings expected")
    };
    (request, saved)
}

async fn start(fixture: &Fixture, query: &Query) -> code::Startup {
    let startup = fixture.read(query.clone(), None).await.startup.unwrap();
    let Output::CodeServer(server) = fixture
        .client
        .execute(fixture.client.prepare(Command::StartCodeServer {
            startup: startup.clone(),
        }))
        .await
        .unwrap()
    else {
        panic!("server expected")
    };
    assert!(matches!(
        server.status,
        code::Status::Ready | code::Status::Indexing
    ));
    startup
}

fn running(pid: &str) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-p", pid, "-o", "stat="])
        .output()
        .unwrap();
    let state = String::from_utf8(output.stdout).unwrap();
    let state = state.trim();
    !state.is_empty() && !state.starts_with('Z')
}

#[tokio::test]
async fn editor_switch_preserves_shared_servers() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let mut fixture = Fixture::new(remote, &server).await;
        fixture.settings(Vec::new(), false).await;
        let mut events = fixture
            .client
            .subscribe_code_servers(fixture.session.worktree)
            .await
            .unwrap();
        let initial = settings(&fixture.client).await;
        assert!(initial.enabled);
        assert!(!initial.editor_enabled);
        assert!(matches!(
            events.next().await.unwrap(),
            sailry_protocol::Update::CodeServers {
                editor_enabled: false,
                ..
            }
        ));
        let query = Query::Hover {
            path: "main.rs".into(),
            line: 1,
            symbol: "symbol".into(),
        };
        start(&fixture, &query).await;
        let pid = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
        let mut enabled = initial;
        enabled.editor_enabled = true;
        let (_, enabled) = save(&fixture.client, enabled).await;
        let update = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let update = events.next().await.unwrap();
                if matches!(
                    update,
                    sailry_protocol::Update::CodeServers {
                        editor_enabled: true,
                        ..
                    }
                ) {
                    break update;
                }
            }
        })
        .await
        .unwrap();
        let wire = serde_json::to_value(&update).unwrap();
        assert_eq!(wire["kind"], json!("code_servers"));
        assert_eq!(wire["data"]["editor_enabled"], json!(true));
        assert!(wire["data"].get("settings").is_none());
        assert!(wire["data"].get("environment").is_none());
        let mut disabled = enabled;
        disabled.editor_enabled = false;
        let (request, disabled) = save(&fixture.client, disabled).await;
        assert!(matches!(
            events.next().await.unwrap(),
            sailry_protocol::Update::CodeServers {
                editor_enabled: false,
                ..
            }
        ));
        assert!(running(&pid));
        assert!(matches!(
            fixture.read(query.clone(), None).await.content,
            Content::Hover(_)
        ));
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::CodeSettings(disabled.clone())
        );
        assert_eq!(settings(&fixture.client).await, disabled);
        drop(events);
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        fixture.node = Node::start_empty_with_environment(
            profile,
            environment(fixture.directory.path(), &fixture.bin),
        )
        .await
        .unwrap();
        let address = fixture
            .controller
            .handle()
            .pair(fixture.node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        fixture.client = Client::new(if remote {
            fixture.controller.handle().remote(address)
        } else {
            fixture.node.local()
        });
        assert_eq!(settings(&fixture.client).await, disabled);
        start(&fixture, &query).await;
        assert!(matches!(
            fixture.read(query, None).await.content,
            Content::Hover(_)
        ));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn stops_servers_and_preserves_management() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        fixture.settings(Vec::new(), false).await;
        let query = Query::Hover {
            path: "main.rs".into(),
            line: 1,
            symbol: "symbol".into(),
        };
        let prepared = fixture.read(query.clone(), None).await.startup.unwrap();
        let mut value = settings(&fixture.client).await;
        assert!(value.enabled);
        assert!(!value.after_edit);
        value.enabled = false;
        let (disable, disabled) = save(&fixture.client, value.clone()).await;
        assert_eq!(disabled.revision, value.revision + 1);
        assert!(!disabled.enabled);
        assert!(!disabled.after_edit);
        for command in [
            Command::ReadCode {
                worktree: fixture.session.worktree,
                query: query.clone(),
                overlay: None,
            },
            Command::ReadCode {
                worktree: fixture.session.worktree,
                query: Query::Symbols {
                    path: None,
                    query: None,
                },
                overlay: None,
            },
            Command::StartCodeServer {
                startup: prepared.clone(),
            },
        ] {
            let error = fixture
                .client
                .execute(fixture.client.prepare(command))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::Unavailable);
            assert_eq!(error.message, "language services are disabled");
        }
        assert!(!fixture.root.join("pid").exists());
        let Output::CodePackages(packages) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCodePackages {
                worktree: fixture.session.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!("packages expected")
        };
        assert!(packages.languages.iter().any(|language| {
            language
                .servers
                .iter()
                .any(|candidate| candidate.server == "rust-analyzer")
        }));
        assert!(matches!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ListCodeInstallations))
                .await
                .unwrap(),
            Output::CodeInstallations(_)
        ));
        assert!(matches!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadCodeRules))
                .await
                .unwrap(),
            Output::CodeRules(_)
        ));
        let mut enabled = disabled.clone();
        enabled.enabled = true;
        let (_, enabled) = save(&fixture.client, enabled).await;
        assert!(!enabled.after_edit);
        // A replay returns its recorded result without applying historical settings.
        assert_eq!(
            fixture.client.execute(disable).await.unwrap(),
            Output::CodeSettings(disabled)
        );
        assert_eq!(settings(&fixture.client).await, enabled);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::SaveCodeSettings {
                    settings: value.clone(),
                    expected_revision: value.revision
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        // Re-enabling requires fresh preparation of the approved descriptor.
        assert_eq!(
            fixture
                .client
                .execute(
                    fixture
                        .client
                        .prepare(Command::StartCodeServer { startup: prepared })
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        start(&fixture, &query).await;
        let Content::Hover(text) = fixture.read(query.clone(), None).await.content else {
            panic!("hover expected")
        };
        assert!(!text.is_empty());
        let pid = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
        assert!(running(&pid));
        let requests = hover_requests(&fixture.root);
        std::fs::write(fixture.root.join("control"), br#"{"hang":true}"#).unwrap();
        let querying = Client::new(if remote {
            fixture
                .controller
                .handle()
                .remote(fixture.node.link().address())
        } else {
            fixture.node.local()
        });
        let request = querying.prepare(Command::ReadCode {
            worktree: fixture.session.worktree,
            query: query.clone(),
            overlay: None,
        });
        let task = tokio::spawn(async move { querying.execute(request).await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while hover_requests(&fixture.root) == requests {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut value = settings(&fixture.client).await;
        value.enabled = false;
        let (_, disabled) = save(&fixture.client, value).await;
        assert!(
            !running(&pid),
            "disable completion must wait for server shutdown"
        );
        assert_eq!(
            task.await.unwrap().unwrap_err().code,
            ErrorCode::Unavailable
        );
        let Output::CodeServers(servers) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCodeServers {
                worktree: fixture.session.worktree,
            }))
            .await
            .unwrap()
        else {
            panic!("servers expected")
        };
        assert!(servers.is_empty());
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadCode {
                    worktree: fixture.session.worktree,
                    query: query.clone(),
                    overlay: None
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        let mut value = disabled;
        value.enabled = true;
        let (_, enabled) = save(&fixture.client, value).await;
        assert!(!enabled.after_edit);
        std::fs::remove_file(fixture.root.join("control")).unwrap();
        start(&fixture, &query).await;
        assert!(matches!(
            fixture.read(query, None).await.content,
            Content::Hover(_)
        ));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

fn hover_requests(root: &std::path::Path) -> usize {
    std::fs::read_to_string(root.join("messages"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|value| value["method"] == "textDocument/hover")
        .count()
}

#[tokio::test]
async fn mounts_queries_only_for_enabled_turns() {
    for remote in [false, true] {
        let server = Server::start(false).await;
        let mut fixture = Fixture::new(remote, &server).await;
        fixture.config(WorkMode::Code, Permission::Full).await;
        for enabled in [true, false, true] {
            let mut value = settings(&fixture.client).await;
            value.enabled = enabled;
            assert!(!value.editor_enabled);
            save(&fixture.client, value).await;
            let turn = fixture.submit().await;
            let page = fixture.finish(turn).await;
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == turn)
                    .unwrap()
                    .status,
                Status::Completed
            );
            let requests = server.requests.lock().unwrap().clone();
            let tools = requests.last().unwrap()["tools"].as_array().unwrap();
            for name in [
                "code_diagnostics",
                "code_definition",
                "code_references",
                "code_hover",
                "code_symbols",
            ] {
                let name = plugin_tool("code", name);
                assert_eq!(
                    tools
                        .iter()
                        .any(|tool| tool["function"]["name"] == name || tool["name"] == name),
                    enabled
                );
            }
            let Output::Session(session) = fixture
                .client
                .execute(fixture.client.prepare(Command::ReadSession {
                    session: fixture.session.id,
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            fixture.session = session;
        }
        assert!(!fixture.root.join("pid").exists());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

async fn package(client: &Client) -> plugin::Info {
    let Output::Plugin(info) = client
        .execute(client.prepare(Command::ReadPlugin {
            name: "code".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("package expected")
    };
    info
}

async fn toggle(client: &Client, enabled: bool) -> (Request, plugin::Info) {
    let current = package(client).await;
    let request = client.prepare(Command::SetPluginEnabled {
        name: "code".into(),
        expected_revision: current.summary.revision,
        enabled,
    });
    let Output::Plugin(info) = client.execute(request.clone()).await.unwrap() else {
        panic!("package expected")
    };
    assert_eq!(info.summary.revision, current.summary.revision + 1);
    assert_eq!(
        info.summary.settings_revision,
        current.summary.settings_revision
    );
    (request, info)
}

#[tokio::test]
async fn synchronizes_plugin_switches_with_settings() {
    for remote in [false, true] {
        let server = Server::start(false).await;
        let fixture = Fixture::new(remote, &server).await;
        let other = Fixture::new(!remote, &server).await;
        let other_settings = settings(&other.client).await;
        let other_package = package(&other.client).await;
        fixture.settings(Vec::new(), false).await;
        let query = Query::Hover {
            path: "main.rs".into(),
            line: 1,
            symbol: "symbol".into(),
        };
        start(&fixture, &query).await;
        let before = settings(&fixture.client).await;
        let pid = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
        let (disable, disabled_package) = toggle(&fixture.client, false).await;
        let disabled = settings(&fixture.client).await;
        assert!(!running(&pid));
        assert!(!disabled.enabled);
        assert!(!disabled.after_edit);
        assert_eq!(disabled.revision, before.revision + 1);
        assert!(!disabled_package.summary.enabled);
        let Output::Plugins(inventory) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListPlugins))
            .await
            .unwrap()
        else {
            panic!("inventory expected")
        };
        assert_eq!(
            inventory.iter().find(|summary| summary.name == "code"),
            Some(&disabled_package.summary)
        );
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadCode {
                    worktree: fixture.session.worktree,
                    query: query.clone(),
                    overlay: None
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        assert_eq!(settings(&other.client).await, other_settings);
        assert_eq!(package(&other.client).await, other_package);
        let mut enabled = disabled;
        enabled.enabled = true;
        let (_, enabled) = save(&fixture.client, enabled).await;
        let enabled_package = package(&fixture.client).await;
        assert!(enabled_package.summary.enabled);
        assert_eq!(
            enabled_package.summary.revision,
            disabled_package.summary.revision + 1
        );
        assert_eq!(
            fixture.client.execute(disable).await.unwrap(),
            Output::Plugin(disabled_package.clone())
        );
        assert_eq!(settings(&fixture.client).await, enabled);
        assert_eq!(package(&fixture.client).await, enabled_package);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::SetPluginEnabled {
                    name: "code".into(),
                    expected_revision: disabled_package.summary.revision,
                    enabled: false
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(settings(&fixture.client).await, enabled);
        start(&fixture, &query).await;
        assert!(matches!(
            fixture.read(query.clone(), None).await.content,
            Content::Hover(_)
        ));
        let pid = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
        let mut edited = enabled;
        edited.after_edit = true;
        let (_, edited) = save(&fixture.client, edited).await;
        assert_eq!(package(&fixture.client).await, enabled_package);
        let mut disabled = edited;
        disabled.enabled = false;
        let (disable_settings, disabled) = save(&fixture.client, disabled).await;
        assert!(!running(&pid));
        assert!(!package(&fixture.client).await.summary.enabled);
        let (_, resumed) = toggle(&fixture.client, true).await;
        assert!(settings(&fixture.client).await.enabled);
        assert!(settings(&fixture.client).await.after_edit);
        assert_eq!(
            settings(&fixture.client).await.revision,
            disabled.revision + 1
        );
        assert_eq!(
            fixture.client.execute(disable_settings).await.unwrap(),
            Output::CodeSettings(disabled)
        );
        assert_eq!(package(&fixture.client).await, resumed);
        start(&fixture, &query).await;
        assert!(matches!(
            fixture.read(query.clone(), None).await.content,
            Content::Hover(_)
        ));
        let pid = std::fs::read_to_string(fixture.root.join("pid")).unwrap();
        let remove = fixture.client.prepare(Command::RemovePlugin {
            name: "code".into(),
            expected_revision: resumed.summary.revision,
        });
        let removed = fixture.client.execute(remove.clone()).await.unwrap();
        let Output::Plugins(inventory) = &removed else {
            panic!("inventory expected")
        };
        assert!(inventory.iter().all(|summary| summary.name != "code"));
        assert!(!running(&pid));
        assert!(!settings(&fixture.client).await.enabled);
        let removed_settings = settings(&fixture.client).await;
        let mut enabling = removed_settings.clone();
        enabling.enabled = true;
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::SaveCodeSettings {
                    expected_revision: enabling.revision,
                    settings: enabling
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        assert_eq!(settings(&fixture.client).await, removed_settings);
        let install = fixture.client.prepare(Command::InstallBundledPlugin {
            name: "code".into(),
            expected_revision: 0,
        });
        let Output::Plugin(installed) = fixture.client.execute(install.clone()).await.unwrap()
        else {
            panic!("package expected")
        };
        assert!(installed.summary.enabled);
        assert!(settings(&fixture.client).await.enabled);
        assert!(settings(&fixture.client).await.after_edit);
        assert_eq!(fixture.client.execute(remove).await.unwrap(), removed);
        assert_eq!(package(&fixture.client).await, installed);
        start(&fixture, &query).await;
        assert!(matches!(
            fixture.read(query.clone(), None).await.content,
            Content::Hover(_)
        ));
        let (disable, disabled_package) = toggle(&fixture.client, false).await;
        assert_eq!(
            fixture.client.execute(install).await.unwrap(),
            Output::Plugin(installed)
        );
        assert!(!settings(&fixture.client).await.enabled);
        assert_eq!(package(&fixture.client).await, disabled_package);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadCode {
                    worktree: fixture.session.worktree,
                    query,
                    overlay: None
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        fixture.client.execute(disable).await.unwrap();
        assert_eq!(settings(&other.client).await, other_settings);
        assert_eq!(package(&other.client).await, other_package);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
        other.node.shutdown().await.unwrap();
        other.controller.close().await.unwrap();
    }
}
