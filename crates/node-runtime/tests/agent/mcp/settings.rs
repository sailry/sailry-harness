use super::*;
use plugin::settings::{SecretUpdate, State};
use std::collections::BTreeMap;
mod collection;

fn declare(root: &Path) {
    let mut server = peer::config("mcp::peer::stdio_peer", "settings");
    server["env"]["SAILRY_MCP_TOKEN"] = json!("");
    package(root, json!({"native":server}), "settings fixture");
    let path = root.join("package");
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(path.join("plugin.json")).unwrap()).unwrap();
    manifest["extensions"] = json!({"dev.sailry.platform":{
        "api_version":"v1", "actions":[], "settings_schema":"dev.sailry.platform/settings.json"
    }});
    fs::write(path.join("plugin.json"), manifest.to_string()).unwrap();
    fs::write(path.join("dev.sailry.platform/settings.json"), json!({
        "$schema":plugin::settings::SCHEMA, "type":"object", "additionalProperties":false,
        "properties":{
            "label":{"type":"string"},
            "token":{"type":"string", "x-sailry-secret":{"server":"native", "env":"SAILRY_MCP_TOKEN"}}
        }, "required":["label", "token"]
    }).to_string()).unwrap();
}

async fn save(
    fixture: &process::Fixture,
    package: plugin::Reference,
    label: &str,
    update: SecretUpdate,
) -> State {
    let Output::PluginSettings(state) = execute(
        &fixture.client,
        Command::SavePluginSettings {
            package,
            values: BTreeMap::from([("label".into(), json!(label))]),
            secrets: BTreeMap::from([("token".into(), update)]),
        },
    )
    .await
    else {
        panic!("settings expected")
    };
    state
}

fn verify(page: &Page, label: &str, secret: &str) {
    assert!(!serde_json::to_string(page).unwrap().contains(secret));
    let results = results(page);
    let result = &results.last().unwrap()["output"];
    assert_eq!(result["settings"], json!({"label":label}));
    assert_eq!(
        result["secret_hash"],
        blake3::hash(secret.as_bytes()).to_hex().to_string()
    );
    assert!(Path::new(result["settings_path"].as_str().unwrap()).exists());
}

#[tokio::test]
async fn releases_frozen_projections() {
    for remote in [false, true] {
        let model = Server::turn_tools(vec![(alias("native", "read"), json!({}))]).await;
        let fixture = process::Fixture::new(remote, &model).await;
        isolate(&fixture.client).await;
        declare(&fixture.root);
        let installed = install(&fixture, 0).await;
        let first_secret = "first-private-${PLUGIN_ROOT}-token";
        let second_secret = "second-private-${PLUGIN_DATA}-token";
        let first = save(
            &fixture,
            installed.summary.reference(),
            "Original 中文 🙂",
            SecretUpdate::Replace(Secret::new(first_secret.into())),
        )
        .await;

        let (_, queued) = submit(&fixture, true).await;
        let second = save(
            &fixture,
            first.package.clone(),
            "Updated",
            SecretUpdate::Replace(Secret::new(second_secret.into())),
        )
        .await;
        assert_eq!(queued.plugins, [first.package]);
        execute(
            &fixture.client,
            Command::StartQueuedTurn { turn: queued.id },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        verify(&page, "Original 中文 🙂", first_secret);
        let original = results(&page)[0]["output"]["settings_path"]
            .as_str()
            .unwrap()
            .to_owned();
        // The next turn replaces the pooled connection using the current frozen settings.
        let (_, next) = submit(&fixture, false).await;
        assert_eq!(next.plugins, [second.package]);
        assert_eq!(next.revision, queued.revision);
        let page = finished(&fixture.client, fixture.session.id, next.id).await;
        verify(&page, "Updated", second_secret);
        let updated = results(&page).last().unwrap()["output"]["settings_path"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(!Path::new(&original).exists());
        fixture.node.shutdown().await.unwrap();
        assert!(!Path::new(&updated).exists());
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn imports_execution_node_configuration() {
    for remote in [false, true] {
        let model = Server::tools(vec![(alias("native", "read"), json!({}))]).await;
        let source = process::Fixture::new(false, &model).await;
        let mut target = process::Fixture::new(remote, &model).await;
        isolate(&source.client).await;
        isolate(&target.client).await;
        declare(&source.root);
        declare(&target.root);
        let one = install(&source, 0).await;
        let two = install(&target, 0).await;
        assert_eq!(one.summary.digest, two.summary.digest);
        let original = save(
            &source,
            one.summary.reference(),
            "Source",
            SecretUpdate::Replace(Secret::new("source-private-token".into())),
        )
        .await;
        let destination = save(
            &target,
            two.summary.reference(),
            "Destination",
            SecretUpdate::Replace(Secret::new("target-private-token".into())),
        )
        .await;
        let destination = save(
            &target,
            destination.package,
            "Destination",
            SecretUpdate::Keep,
        )
        .await;
        assert_ne!(
            original.package.settings_revision,
            destination.package.settings_revision
        );
        source
            .node
            .link()
            .pair(target.node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let Output::Snapshot(snapshot) = execute(&source.client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let provider = snapshot
            .providers
            .iter()
            .find(|provider| provider.id == source.session.config.provider)
            .unwrap();
        let request = source.client.prepare(Command::CreateSessionAt {
            target: target.node.id(),
            project: target.session.project,
            worktree: Some(target.session.worktree),
            config: Box::new(source.session.config.clone()),
            provider_revision: provider.revision,
        });
        let Output::Session(imported) = source.client.execute(request.clone()).await.unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(
            source.client.execute(request).await.unwrap(),
            Output::Session(imported.clone())
        );
        // Execution and restoration no longer require the originating controller or Node.
        source.node.shutdown().await.unwrap();
        source.controller.close().await.unwrap();
        target.session = imported;
        let (_, turn) = submit(&target, false).await;
        assert_eq!(turn.plugins, [destination.package]);
        let page = finished(&target.client, target.session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        verify(&page, "Destination", "target-private-token");
        assert!(
            !serde_json::to_string(&page)
                .unwrap()
                .contains("source-private-token")
        );
        target.node.shutdown().await.unwrap();
        target.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_cleared_credentials() {
    for remote in [false, true] {
        let model = Server::tools(vec![(
            "load_skill".into(),
            json!({"skill":"example:analysis"}),
        )])
        .await;
        let fixture = process::Fixture::new(remote, &model).await;
        declare(&fixture.root);
        let installed = install(&fixture, 0).await;
        let configured = save(
            &fixture,
            installed.summary.reference(),
            "Configured",
            SecretUpdate::Replace(Secret::new("revoke-private-token".into())),
        )
        .await;

        let (_, queued) = submit(&fixture, true).await;
        let cleared = save(
            &fixture,
            configured.package,
            "Configured",
            SecretUpdate::Clear,
        )
        .await;
        assert!(!cleared.ready);
        execute(
            &fixture.client,
            Command::StartQueuedTurn { turn: queued.id },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(
            serde_json::to_string(&page)
                .unwrap()
                .contains("Complete skill beside MCP")
        );
        assert!(
            !fixture
                .node
                .profile()
                .join("plugins/data/example/launches")
                .exists()
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn releases_failed_initialization() {
    for remote in [false, true] {
        for mode in ["bad_handshake", "slow_handshake"] {
            let model = Server::tools(vec![(
                "load_skill".into(),
                json!({"skill":"example:analysis"}),
            )])
            .await;
            let fixture = process::Fixture::new(remote, &model).await;
            declare(&fixture.root);
            let path = fixture.root.join("package/mcp.json");
            let mut mcp: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            mcp["mcpServers"]["native"]["env"]["SAILRY_MCP_MODE"] = json!(mode);
            fs::write(path, mcp.to_string()).unwrap();
            let installed = install(&fixture, 0).await;
            save(
                &fixture,
                installed.summary.reference(),
                "Configured",
                SecretUpdate::Replace(Secret::new("fixture-token".into())),
            )
            .await;

            let (_, turn) = submit(&fixture, false).await;
            let data = fixture.node.profile().join("plugins/data/example");
            present(&data.join("pid")).await;
            let projections = || {
                fs::read_dir(&data)
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".sailry-settings-")
                    })
                    .count()
            };
            if mode == "slow_handshake" {
                assert_eq!(projections(), 1);
                execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
            }
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                if mode == "slow_handshake" {
                    Status::Cancelled
                } else {
                    Status::Completed
                }
            );
            reaped(&data);
            assert_eq!(projections(), 0);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
