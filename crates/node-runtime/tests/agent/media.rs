use super::*;
use sailry_protocol::media::{Binding, Kind, Settings};
use serde_json::json;
#[path = "media/follow.rs"]
mod follow;
#[path = "media/server.rs"]
mod server;

async fn install(client: &Client) -> plugin::Info {
    let Output::Plugin(package) = client
        .execute(client.prepare(Command::InstallBundledPlugin {
            name: "media".into(),
            expected_revision: 0,
        }))
        .await
        .unwrap()
    else {
        panic!("media package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    assert!(package.summary.enabled);
    assert!(package.skills.iter().any(|skill| skill.name == "imagegen"));
    package
}

pub(super) fn model(id: &str) -> Model {
    Model {
        id: id.into(),
        display_name: None,
        context: 16000,
        output: 2048,
        vision: true,
        tools: true,
        reasoning: false,
        web_search: false,
        generates: vec![],
        efforts: vec![],
        capabilities: None,
        custom_efforts: false,
        compaction_tokens: None,
        default_effort: Effort::Default,
    }
}

async fn replace_package(client: &Client, root: &std::path::Path, worktree: WorktreeId) {
    let Output::Plugin(package) = client
        .execute(client.prepare(Command::ReadPlugin {
            name: "media".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("media package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    assert!(package.summary.enabled);
    assert!(!package.summary.digest.is_empty());
    let extension = package.extension.as_ref().unwrap();
    let host = extension.host.as_ref().unwrap();
    let resources: std::collections::BTreeSet<_> = host
        .resources
        .iter()
        .chain(&extension.desktop.as_ref().unwrap().resources)
        .map(String::as_str)
        .chain(["plugin.json"])
        .collect();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/media");
    let target = root.join("media-update");
    for resource in resources {
        let destination = target.join(resource);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::copy(source.join(resource), destination).unwrap();
    }
    let entry = target.join(&host.entry);
    let mut script = std::fs::read_to_string(&entry).unwrap();
    script
        .push_str("\nthrow new Error('updated handlers must not replace an admitted package');\n");
    std::fs::write(entry, script).unwrap();
    let Output::Plugin(updated) = client
        .execute(client.prepare(Command::InstallPlugin {
            worktree,
            path: "media-update".into(),
            name: "media".into(),
            expected_revision: package.summary.revision,
            expected_digest: None,
        }))
        .await
        .unwrap()
    else {
        panic!("updated media package expected")
    };
    assert!(updated.issues.is_empty(), "{:?}", updated.issues);
    assert_ne!(updated.summary.digest, package.summary.digest);
}

#[tokio::test]
async fn restores_frozen_execution() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("input.png"), server::image()).unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
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
        install(&client).await;
        let server = Server::tools(vec![
            (
                plugin_tool("media", "inspect_image"),
                json!({"prompt":"Describe this image", "path":"input.png"}),
            ),
            (
                plugin_tool("media", "generate_image"),
                json!({"prompt":"A pixel", "file_name":"image.png"}),
            ),
            (
                plugin_tool("media", "generate_video"),
                json!({"prompt":"A pixel moving", "file_name":"video.mp4"}),
            ),
            (
                plugin_tool("media", "generate_image"),
                json!({"prompt":"Do not replace", "file_name":"image.png"}),
            ),
            (
                plugin_tool("media", "generate_image"),
                json!({"prompt":"Invalid artifact", "file_name":"invalid.png"}),
            ),
        ])
        .await;
        let (base, mut provider) = configured_session(&client, &server.endpoint, &root).await;
        provider.models = vec![model("fixture-a")];
        provider.models[0].vision = false;
        provider.models[0].capabilities =
            Some(sailry_protocol::conversation::discovery::Capabilities {
                vision: Some(false),
                ..Default::default()
            });
        provider.default_model = "fixture-a".into();
        let Output::Provider(saved) = client
            .execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: provider.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        assert!(!saved.models[0].vision);
        provider = saved;
        let media = server::Server::start().await;
        provider.id = ProviderId::new();
        provider.revision = 0;
        provider.endpoint = media.endpoint.clone();
        provider.models[0].vision = true;
        provider.models[0].capabilities.as_mut().unwrap().vision = Some(true);
        let catalog = serde_json::to_vec(&json!({"version":1,"partners":[{
            "id":"media-fixture","name":"Media fixture",
            "routes":[{"protocol":"openai-chat-completions","endpoint":media.endpoint}],
            "auth":{"kind":"api_key","data":{"header":"X-Media-Key","prefix":"Token "}}
        }]}))
        .unwrap();
        node.with_storage(move |db| Box::pin(async move {
            db.execute("INSERT INTO channel_presets(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [catalog]).await.map_err(crate::database_fixture::fault)?;
            Ok(())
        })).await.unwrap();
        provider.preset = Some(sailry_protocol::conversation::presets::PresetRef {
            source: sailry_protocol::conversation::presets::Source::Partner,
            id: "media-fixture".into(),
            route: None,
        });
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::SaveProvider {
                provider,
                expected_revision: 0,
                secret: Some(Secret::new("isolated-media-key".into())),
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        assert!(provider.models[0].vision);
        let settings = Settings {
            revision: 0,
            bindings: [Kind::Vision, Kind::Image, Kind::Video]
                .into_iter()
                .map(|kind| {
                    (
                        kind,
                        Some(Binding {
                            provider: provider.id,
                            model: "fixture-a".into(),
                        }),
                    )
                })
                .collect(),
        };
        let request = client.prepare(Command::SaveMediaSettings(settings.clone()));
        let saved = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), saved);
        assert_eq!(
            client
                .execute(client.prepare(Command::SaveMediaSettings(settings)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let mut config = base.config.clone();
        config.permission = Permission::Full;
        config.effort = Effort::Default;
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: base.project,
                worktree: Some(base.worktree),
                config: Some(config),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let request = client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: 1,
            message: "Use the configured media tools".into(),
        });
        let Output::QueuedTurn(turn) = client.execute(request.clone()).await.unwrap() else {
            panic!("turn expected")
        };
        replace_package(&client, &root, session.worktree).await;
        client
            .execute(client.prepare(Command::SaveMediaSettings(Settings {
                revision: 1,
                bindings: Default::default(),
            })))
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::RemoveProvider {
                provider: provider.id,
                expected_revision: provider.revision,
            }))
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::StartQueuedTurn { turn: turn.id }))
            .await
            .unwrap();
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(
            page.runs[0].status,
            Status::Completed,
            "{:?}",
            page.runs[0].error
        );
        let calls: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolCall { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 5);
        assert!(
            calls
                .iter()
                .all(|name| name.starts_with(&plugin_tool("media", "")))
        );
        let results: Vec<_> = results(&client, &page).await;
        assert_eq!(results.len(), 5);
        assert_eq!(results[0]["text"], "One bright pixel", "{results:?}");
        assert_eq!(
            results[1]["path"], "assets/generated/image.png",
            "{results:?}"
        );
        assert_eq!(
            results[2]["path"], "assets/generated/video.mp4",
            "{results:?}"
        );
        assert_eq!(results[3]["error"]["code"], "revision_conflict");
        assert_eq!(results[4]["error"]["code"], "unavailable");
        assert!(!root.join("assets/generated/invalid.png").exists());
        let usage_query = sailry_protocol::usage::Query {
            start_ms: chrono::Utc::now().timestamp_millis() - 60_000,
            end_ms: chrono::Utc::now().timestamp_millis() + 60_000,
            dimension: sailry_protocol::usage::Dimension::Model,
            utc_offset_minutes: 0,
            projects: vec![session.project.unwrap()],
            worktrees: vec![session.worktree],
            providers: vec![provider.id],
            models: vec!["fixture-a".into()],
            before: None,
        };
        let Output::Usage(usage) = client
            .execute(client.prepare(Command::ReadUsage(usage_query.clone())))
            .await
            .unwrap()
        else {
            panic!("usage expected")
        };
        assert_eq!(usage.totals.responses, 3);
        assert_eq!(usage.totals.tokens.as_ref().unwrap().input, 105);
        assert_eq!(usage.totals.tokens.as_ref().unwrap().output, 124);
        assert_eq!(usage.totals.generation.as_ref().unwrap().responses, 1);
        assert_eq!(usage.totals.generation.as_ref().unwrap().output_tokens, 4);
        assert!(usage.totals.cost.is_none());
        assert_eq!(usage.groups.len(), 1);
        assert_eq!(
            usage.groups[0].key,
            sailry_protocol::usage::Key::Model {
                provider: provider.id,
                model: "fixture-a".into()
            }
        );
        assert_eq!(
            std::fs::read(root.join("assets/generated/image.png")).unwrap(),
            server::image()
        );
        assert!(root.join("assets/generated/video.mp4").is_file());
        let records = media.requests.lock().unwrap().clone();
        let headers = media.headers.lock().unwrap().clone();
        assert_eq!(headers.len(), records.len());
        for ((path, _), headers) in records.iter().zip(headers) {
            assert!(!headers.contains_key("authorization"));
            assert!(!headers.contains_key("x-api-key"));
            if path == "/v1/generated.png" {
                assert!(
                    !headers.contains_key("x-media-key"),
                    "generated URLs must not receive provider credentials"
                );
            } else {
                assert_eq!(headers["x-media-key"], "Token isolated-media-key", "{path}");
            }
        }
        assert_eq!(
            records
                .iter()
                .filter(|(path, _)| path == "/v1/images/generations")
                .count(),
            2
        );
        assert_eq!(
            records
                .iter()
                .filter(|(path, _)| path == "/v1/videos")
                .count(),
            1
        );
        assert!(
            records
                .iter()
                .filter(|(_, body)| !body.is_null())
                .all(|(_, body)| body["model"] == "fixture-a")
        );
        let users = records[0].1["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .collect::<Vec<_>>();
        assert_eq!(users.len(), 1);
        assert!(
            users[0]["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|part| part["type"] == "image_url")
        );
        let Output::Session(fork) = client
            .execute(client.prepare(Command::ForkConversation {
                session: session.id,
                through: turn.id,
                expected_revision: 1,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected")
        };
        let session_id = session.id;
        let fork_id = fork.id;
        let (original, copied): (Vec<u8>, Vec<u8>) = node
            .with_storage(move |db| {
                Box::pin(async move {
                    let original = db
                        .prepare("SELECT body FROM session_media WHERE session=?1")
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .query_row([session_id.to_string()])
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .get(0)
                        .map_err(crate::database_fixture::fault)?;
                    let copied = db
                        .prepare("SELECT body FROM session_media WHERE session=?1")
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .query_row([fork_id.to_string()])
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .get(0)
                        .map_err(crate::database_fixture::fault)?;
                    Ok((original, copied))
                })
            })
            .await
            .unwrap();
        assert_eq!(original, copied);
        node.shutdown().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(history(&client, session.id).await.entries, page.entries);
        let Output::Usage(restored_usage) = client
            .execute(client.prepare(Command::ReadUsage(usage_query)))
            .await
            .unwrap()
        else {
            panic!("usage expected")
        };
        assert_eq!(restored_usage.totals, usage.totals);
        assert_eq!(restored_usage.groups, usage.groups);
        assert_eq!(media.requests.lock().unwrap().len(), records.len());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
