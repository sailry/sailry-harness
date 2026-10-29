use super::*;
use sailry_protocol::{
    conversation::{
        Model, Provider,
        presets::{PresetRef, Source},
    },
    media::{Binding, Kind, Settings},
};

fn declare(root: &Path, version: u32) {
    fs::write(root.join("plugin.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name":"example", "version":format!("0.1.{version}"), "description":"Media retention fixture",
        "extensions":{"dev.sailry.platform":{
            "api_version":"v1", "settings_schema":"dev.sailry.platform/settings.json",
            "model_providers":[{"id":"example","name":"Media fallback","routes":[{
                "protocol":"openai-chat-completions","endpoint":"http://127.0.0.1:9/v1"
            }]}]
        }}
    }).to_string()).unwrap();
    fs::write(
        root.join("dev.sailry.platform/settings.json"),
        json!({
            "$schema":plugin::settings::SCHEMA, "type":"object", "additionalProperties":false,
            "properties":{"endpoint":{"type":"string"}}, "required":["endpoint"]
        })
        .to_string(),
    )
    .unwrap();
}

async fn save(
    client: &Client,
    package: &plugin::Reference,
    endpoint: &str,
) -> plugin::settings::State {
    state(
        execute(
            client,
            Command::SavePluginSettings {
                package: package.clone(),
                values: [("endpoint".into(), json!(endpoint))].into(),
                secrets: Default::default(),
            },
        )
        .await,
    )
}

async fn resources(
    node: &Node,
    session: SessionId,
    package: &plugin::Reference,
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let package = package.clone();
    node.with_storage(move |db| Box::pin(async move {
        let row = db.prepare("SELECT (SELECT body FROM session_media WHERE session=?1),(SELECT body FROM plugin_packages WHERE digest=?2 AND name=?3),(SELECT body FROM plugin_settings WHERE plugin=?3 AND revision=?4)")
            .await.map_err(crate::plugins::database_fixture::fault)?
            .query_row(turso::params![session.to_string(), package.digest, package.name, package.settings_revision as i64])
            .await.map_err(crate::plugins::database_fixture::fault)?;
        let values: turso::Result<_> = (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))(&row);
        values.map_err(crate::plugins::database_fixture::fault)
    })).await.unwrap()
}

#[tokio::test]
async fn retains_frozen_fallbacks_after_updates() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let source = directory.path().join("source/package");
        fs::remove_dir_all(source.join("skills")).unwrap();
        declare(&source, 0);
        let installed = info(execute(&client, install(worktree, 0)).await);
        assert!(installed.issues.is_empty(), "{:?}", installed.issues);
        assert!(installed.skills.is_empty());
        assert!(installed.mcp.is_empty());
        let extension = installed.extension.as_ref().unwrap();
        assert!(extension.tools.is_empty());
        let first = save(
            &client,
            &installed.summary.reference(),
            "https://first.example.com",
        )
        .await;
        let primary: Provider = serde_json::from_value(json!({
            "id":ProviderId::new(), "revision":0, "name":"Native primary", "api":"chat_completions",
            "authentication":"api_key", "endpoint":"http://127.0.0.1:9/v1", "enabled":true,
            "models":[], "default_model":"fixture", "credential":null
        }))
        .unwrap();
        let Output::Provider(primary) = execute(
            &client,
            Command::PutProvider {
                provider: primary,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("primary provider expected")
        };
        let mut fallback = primary.clone();
        fallback.id = ProviderId::new();
        fallback.revision = 0;
        fallback.name = "Package fallback".into();
        fallback.preset = Some(PresetRef {
            source: Source::Package {
                name: "example".into(),
            },
            id: "example".into(),
            route: None,
        });
        fallback.models = vec![Model {
            id: "fixture".into(),
            display_name: None,
            context: 4096,
            output: 128,
            vision: true,
            tools: false,
            reasoning: false,
            web_search: false,
            generates: vec![],
            efforts: vec![],
            capabilities: None,
            custom_efforts: false,
            compaction_tokens: None,
            default_effort: Effort::Default,
        }];
        let Output::Provider(fallback) = execute(
            &client,
            Command::PutProvider {
                provider: fallback,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("fallback provider expected")
        };
        execute(
            &client,
            Command::SaveMediaSettings(Settings {
                revision: 0,
                bindings: [
                    (
                        Kind::Vision,
                        Some(Binding {
                            provider: fallback.id,
                            model: "fixture".into(),
                        }),
                    ),
                    (Kind::Image, None),
                ]
                .into(),
            }),
        )
        .await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let project = snapshot
            .worktrees
            .iter()
            .find(|item| item.id == worktree)
            .unwrap()
            .project;
        let Output::Session(session) = execute(
            &client,
            Command::CreateSession {
                project,
                worktree: Some(worktree),
                config: Some(SessionConfig {
                    provider: primary.id,
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    permission: Permission::Ask,
                    credential: None,
                    assistant: None,
                    resource: None,
                    features: Default::default(),
                    options: Default::default(),
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let frozen = resources(&node, session.id, &first.package).await;
        let media: serde_json::Value = serde_json::from_slice(&frozen.0).unwrap();
        assert_eq!(
            media["vision"]["provider"]["binding"]["package"],
            json!(first.package)
        );
        assert_eq!(media["image"], serde_json::Value::Null);
        let settings: serde_json::Value = serde_json::from_slice(&frozen.2).unwrap();
        assert_eq!(settings["values"]["endpoint"], "https://first.example.com");
        let second = save(&client, &first.package, "https://unused.example.com").await;
        let current = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "example".into(),
                },
            )
            .await,
        );
        declare(&source, 1);
        let unused = info(execute(&client, install(worktree, current.summary.revision)).await);
        declare(&source, 2);
        let updated = info(execute(&client, install(worktree, unused.summary.revision)).await);
        let latest = save(
            &client,
            &updated.summary.reference(),
            "https://latest.example.com",
        )
        .await;
        assert_eq!(first.package.settings_revision, 1);
        assert_eq!(second.package.settings_revision, 2);
        assert_eq!(latest.package.settings_revision, 3);
        assert_ne!(unused.summary.digest, first.package.digest);
        assert_ne!(updated.summary.digest, unused.summary.digest);
        assert_eq!(counts(&node).await, (3, 3, 0));
        let profile = node.profile().to_owned();
        let packages = profile.join("plugins/packages");
        node.shutdown().await.unwrap();
        assert_eq!(closed(&profile).await, (2, 2, 0));
        assert!(packages.join(&first.package.digest).is_dir());
        assert!(packages.join(&latest.package.digest).is_dir());
        assert!(!packages.join(&unused.summary.digest).exists());
        let node = Node::start_empty_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(resources(&node, session.id, &first.package).await, frozen);
        assert_eq!(read(&client, &latest.package).await, latest);
        assert_eq!(counts(&node).await, (2, 2, 0));
        node.shutdown().await.unwrap();
        assert_eq!(closed(&profile).await, (2, 2, 0));
        assert!(packages.join(&first.package.digest).is_dir());
        assert!(!packages.join(&unused.summary.digest).exists());
        controller.close().await.unwrap();
    }
}
