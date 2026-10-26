use super::*;
use plugin::{mcp::Definition, settings::SecretUpdate};
use std::collections::BTreeMap;

#[tokio::test]
async fn manages_only_standalone_inventory() {
    for remote in [false, true] {
        let model = Server::scripted_tools(Vec::new()).await;
        let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
        super::super::planning::configure(&mut fixture, WorkMode::Code).await;
        let definition = Definition::StreamableHttp {
            url: "https://example.invalid/mcp".into(),
            headers: vec!["Authorization".into()],
        };
        let Output::Plugin(installed) = execute(
            &fixture.client,
            Command::InstallMcp {
                name: "mcp-private".into(),
                expected_revision: 0,
                definition,
                secrets: BTreeMap::from([("Authorization".into(), SecretUpdate::Clear)]),
            },
        )
        .await
        else {
            panic!("MCP package expected")
        };
        let field = installed
            .settings
            .as_ref()
            .unwrap()
            .properties
            .keys()
            .next()
            .unwrap()
            .clone();
        let private = "isolated-management-fixture-secret";
        execute(
            &fixture.client,
            Command::SavePluginSettings {
                package: installed.summary.reference(),
                values: BTreeMap::new(),
                secrets: BTreeMap::from([(
                    field.clone(),
                    SecretUpdate::Replace(Secret::new(private.into())),
                )]),
            },
        )
        .await;
        let Output::Plugin(configured) = execute(
            &fixture.client,
            Command::ReadPlugin {
                name: "mcp-private".into(),
            },
        )
        .await
        else {
            panic!("MCP package expected")
        };
        let Output::Plugin(disabled) = execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: "mcp-private".into(),
                expected_revision: configured.summary.revision,
                enabled: false,
            },
        )
        .await
        else {
            panic!("MCP package expected")
        };
        package(
            &fixture.root,
            json!({"feature":{"type":"streamable-http","url":"https://example.invalid/feature"}}),
            "feature fixture",
        );
        let manifest_path = fixture.root.join("package/plugin.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["name"] = json!("mcp-feature");
        fs::write(manifest_path, manifest.to_string()).unwrap();
        let Output::Plugin(feature) = execute(
            &fixture.client,
            Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "package".into(),
                name: "mcp-feature".into(),
                expected_revision: 0,
                expected_digest: None,
            },
        )
        .await
        else {
            panic!("feature package expected")
        };
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: feature.summary.name.clone(),
                expected_revision: feature.summary.revision,
                enabled: false,
            },
        )
        .await;
        let Output::Plugin(feature) = execute(
            &fixture.client,
            Command::ReadPlugin {
                name: "mcp-feature".into(),
            },
        )
        .await
        else {
            panic!("feature package expected")
        };
        model.replace_tools(vec![
            ("manage_mcp".into(), json!({"action":"list"})),
            ("manage_mcp".into(), json!({"action":"update","name":"mcp-private","expected_revision":disabled.summary.revision})),
            ("manage_mcp".into(), json!({"action":"install","name":"mcp-new","definition":{"type":"streamable-http","url":"https://example.invalid/new","headers":["API-Key"]}})),
            ("manage_mcp".into(), json!({"action":"uninstall","name":"mcp-feature","expected_revision":feature.summary.revision})),
            ("manage_mcp".into(), json!({"action":"uninstall","name":"mcp-new","expected_revision":2})),
            ("manage_mcp".into(), json!({"action":"list"})),
        ]);
        let (_, turn) = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        let outputs = results(&fixture.client, &page).await;
        assert_eq!(outputs.len(), 6);
        let view = super::super::progress::observe(&fixture.client, fixture.session.id).await;
        assert_eq!(view.calls.len(), 6);
        for (call, details) in view
            .calls
            .iter()
            .zip([false, true, true, true, true, false])
        {
            assert_eq!(
                call.presentation,
                sailry_protocol::tool::Presentation::Details
            );
            assert_eq!(call.details(&page), details);
            assert!(call.display(&page).unwrap().valid());
        }
        assert_eq!(outputs[1]["enabled"], false);
        assert_eq!(outputs[1]["revision"], disabled.summary.revision + 2);
        assert_eq!(outputs[2]["credential_slots"], json!(["API-Key"]));
        assert_eq!(
            outputs[3]["error"]["code"],
            json!(ErrorCode::PermissionDenied)
        );
        assert_eq!(outputs[4]["servers"].as_array().unwrap().len(), 2);
        assert_eq!(outputs[5]["servers"].as_array().unwrap().len(), 2);
        assert!(!serde_json::to_string(&page).unwrap().contains(private));
        for request in model.requests.lock().unwrap().iter() {
            assert!(!request.to_string().contains(private));
            let names: Vec<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["function"]["name"].as_str().unwrap())
                .collect();
            assert!(names.contains(&"manage_skills"));
            assert!(names.contains(&"manage_mcp"));
            for name in [
                "list_mcp_servers",
                "install_mcp_server",
                "update_mcp_server",
                "uninstall_mcp_server",
            ] {
                assert!(!names.contains(&name));
            }
        }
        let Output::Plugin(current) = execute(
            &fixture.client,
            Command::ReadPlugin {
                name: "mcp-private".into(),
            },
        )
        .await
        else {
            panic!("MCP package expected")
        };
        assert!(!current.summary.enabled);
        assert_eq!(
            current
                .settings
                .as_ref()
                .unwrap()
                .properties
                .keys()
                .collect::<Vec<_>>(),
            [&field]
        );
        let package = current.summary.reference();
        let body: Vec<u8> = fixture
            .node
            .with_storage(move |db| {
                Box::pin(async move {
                    db.prepare("SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2")
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .query_row(turso::params![
                            package.name,
                            i64::try_from(package.settings_revision).unwrap()
                        ])
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .get::<Vec<u8>>(0)
                        .map_err(crate::database_fixture::fault)
                })
            })
            .await
            .unwrap();
        let saved: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(saved["slots"][&field], private);
        assert!(feature.mcp_source.is_none());
        assert!(
            !fixture
                .node
                .profile()
                .join("plugins/data/mcp-feature/pid")
                .exists()
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn planning_exposes_only_inventory() {
    for remote in [false, true] {
        let model = Server::tools(vec![("manage_mcp".into(), json!({"action":"list"}))]).await;
        let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
        super::super::planning::configure(&mut fixture, WorkMode::Plan).await;
        let (_, turn) = submit(&fixture, false).await;
        let page = questions::finish_plan(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert!(page.approvals.is_empty());
        assert_eq!(
            results(&fixture.client, &page).await[0],
            json!({"servers":[]})
        );
        for request in model.requests.lock().unwrap().iter() {
            let names: Vec<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|tool| tool["function"]["name"].as_str().unwrap())
                .collect();
            assert!(names.contains(&"manage_mcp"));
            assert!(names.contains(&"manage_skills"));
            for name in [
                "install_mcp_server",
                "update_mcp_server",
                "uninstall_mcp_server",
            ] {
                assert!(!names.contains(&name));
            }
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
