use super::*;
use sailry_protocol::media::{Binding, Generation, Kind, Settings};
use serde_json::json;
#[path = "media/server.rs"]
mod server;

pub(super) fn model(id: &str) -> Model {
    Model {
        id: id.into(),
        context: 16000,
        output: 2048,
        vision: true,
        tools: true,
        reasoning: false,
        web_search: false,
        generates: vec![Generation::Image, Generation::Video],
        efforts: vec![],
        custom_efforts: false,
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
        let node = Node::start(&profile).await.unwrap();
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
        let (base, mut provider) = configured(&client, &server.endpoint, &root).await;
        provider.models = vec![model("fixture-a")];
        provider.default_model = "fixture-a".into();
        client
            .execute(client.prepare(Command::PutProvider {
                provider: provider.clone(),
                expected_revision: provider.revision,
            }))
            .await
            .unwrap();
        let media = server::Server::start().await;
        provider.id = ProviderId::new();
        provider.revision = 0;
        provider.endpoint = media.endpoint.clone();
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::PutProvider {
                provider,
                expected_revision: 0,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let settings = Settings {
            revision: 0,
            bindings: [Kind::Vision, Kind::Image, Kind::Video]
                .into_iter()
                .map(|kind| {
                    (
                        kind,
                        Binding {
                            provider: provider.id,
                            model: "fixture-a".into(),
                        },
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
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .collect();
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
        assert!(
            records[0].1["messages"][0]["content"]
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
        let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let original: Vec<u8> = db
            .query_row(
                "SELECT body FROM session_media WHERE session=?1",
                [session.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let copied: Vec<u8> = db
            .query_row(
                "SELECT body FROM session_media WHERE session=?1",
                [fork.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(original, copied);
        drop(db);
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
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
