use sailry_client::{Apply, Client, Projection};
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    plugin::{Info, Summary},
    *,
};
use std::{fs, path::Path, time::Duration};
use support::{execute, fixture, info, install, package};
#[path = "plugins/actions.rs"]
mod actions;
#[path = "plugins/catalog/mod.rs"]
mod catalog;
#[path = "plugins/completion.rs"]
mod completion;
#[path = "plugins/dispatch.rs"]
mod dispatch;
#[path = "plugins/distribution.rs"]
mod distribution;
#[path = "plugins/host.rs"]
mod host;
#[path = "plugins/http.rs"]
mod http;
#[path = "plugins/inventory.rs"]
mod inventory;
#[path = "plugins/mcp_configuration.rs"]
mod mcp_configuration;
#[allow(dead_code)]
#[path = "agent_support/mod.rs"]
mod model_fixture;
#[path = "plugins/notifications.rs"]
mod notifications;
#[path = "plugins/reminders_host.rs"]
mod reminders_host;
#[path = "plugins/scheduled_host.rs"]
mod scheduled_host;
#[path = "plugins/settings.rs"]
mod settings;
#[cfg(feature = "test-support")]
#[path = "plugins/skills.rs"]
mod skills;
#[path = "support/skills_git.rs"]
mod source;
#[path = "plugins/standalone_mcp.rs"]
mod standalone_mcp;
#[path = "plugins/storage.rs"]
mod storage;
#[path = "plugins/support.rs"]
pub(super) mod support;
#[path = "plugins/transactions.rs"]
mod transactions;
#[path = "plugins/updates.rs"]
mod updates;
#[path = "plugins/upload.rs"]
mod upload;

async fn inventory(client: &Client) -> Vec<Summary> {
    let Output::Plugins(plugins) = execute(client, Command::ListPlugins).await else {
        panic!("plugin list expected")
    };
    let default = plugins
        .iter()
        .find(|entry| entry.name == "reminders")
        .expect("default package");
    assert!(default.enabled && default.digest.len() == 64);
    // These fixtures compare explicitly installed test packages. Distribution
    // and removal of defaults have their own local and remote acceptance test.
    plugins
        .into_iter()
        .filter(|entry| {
            ![
                "reminders",
                "scheduled-tasks",
                "web-search",
                "goals",
                "commands",
                "external-browser",
                "browser",
                "computer",
                "media",
                "memory",
                "files",
                "progress",
                "delegation",
                "statistics",
                "databases",
                "ssh",
                "worktrees",
                "git",
                "context7",
                "github",
                "code-review",
                "office",
            ]
            .contains(&entry.name.as_str())
        })
        .collect()
}

#[tokio::test]
async fn recovers_installation() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let mut updates = client.subscribe().await.unwrap();
        let mut projection = Projection::new(node.id(), 1);
        assert_eq!(
            projection.apply(1, updates.next().await.unwrap()).unwrap(),
            Apply::Applied
        );
        let defaults = projection.snapshot().unwrap().plugins.clone();
        let request = client.prepare(install(worktree, 0));
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending);
        let first = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&request).await.unwrap() {
                    break info(result.unwrap());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(first.summary.revision, 1);
        assert_eq!(
            first.origin,
            Some(plugin::Origin::Worktree {
                worktree,
                path: "package".into()
            })
        );
        assert_eq!(first.skills.len(), 1);
        assert_eq!(first.summary.description.as_deref(), Some("分析数据 🙂"));
        assert_eq!(
            projection.apply(1, updates.next().await.unwrap()).unwrap(),
            Apply::Applied
        );
        let mut expected = std::iter::once(first.summary.clone())
            .chain(defaults)
            .collect::<Vec<_>>();
        expected.sort_by(|left, right| left.name.cmp(&right.name));
        assert_eq!(projection.snapshot().unwrap().plugins.as_slice(), expected);
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: 1,
                    enabled: false,
                },
            )
            .await,
        );
        assert!(!disabled.summary.enabled);
        assert_eq!(disabled.summary.revision, 2);
        package(&directory.path().join("source/package"), "2.0.0");
        let updated = info(execute(&client, install(worktree, 2)).await);
        assert!(!updated.summary.enabled);
        assert_eq!(updated.summary.revision, 3);
        assert_ne!(updated.summary.digest, first.summary.digest);
        assert_eq!(info(client.execute(request.clone()).await.unwrap()), first);
        assert_eq!(
            inventory(&client).await.as_slice(),
            std::slice::from_ref(&updated.summary)
        );
        assert_eq!(
            client
                .execute(client.prepare(install(worktree, 1)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let stored = node
            .profile()
            .join("plugins/packages")
            .join(&first.summary.digest);
        assert_eq!(
            fs::read_to_string(stored.join("skills/analysis/references/guide.md")).unwrap(),
            "1.0.0"
        );
        let node_id = node.id();
        drop(updates);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        assert_eq!(node.id(), node_id);
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            inventory(&client).await.as_slice(),
            std::slice::from_ref(&updated.summary)
        );
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
            updated
        );
        assert_eq!(info(client.execute(request).await.unwrap()), first);
        let empty = Node::start(directory.path().join("other-node"))
            .await
            .unwrap();
        assert!(inventory(&Client::new(empty.local())).await.is_empty());
        empty.shutdown().await.unwrap();
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn retains_removed_resources() {
    for remote in [false, true] {
        let (_directory, node, controller, client, worktree) = fixture(remote).await;
        let defaults = execute(&client, Command::ListPlugins).await;
        let first = info(execute(&client, install(worktree, 0)).await);
        let remove = client.prepare(Command::RemovePlugin {
            name: "example".into(),
            expected_revision: 1,
        });
        assert_eq!(client.execute(remove.clone()).await.unwrap(), defaults);
        assert!(inventory(&client).await.is_empty());
        let installed = info(execute(&client, install(worktree, 0)).await);
        assert_eq!(installed.summary.revision, 3);
        assert_eq!(installed.summary.digest, first.summary.digest);
        assert_eq!(client.execute(remove).await.unwrap(), defaults);
        assert_eq!(inventory(&client).await, [installed.summary]);
        assert_eq!(
            client
                .execute(client.prepare(Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: 1,
                    enabled: false
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert!(
            node.profile()
                .join("plugins/packages")
                .join(first.summary.digest)
                .join("plugin.json")
                .is_file()
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn isolates_resources() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let source = directory.path().join("source/package");
        fs::create_dir_all(source.join("skills/invalid")).unwrap();
        fs::write(
            source.join("skills/invalid/SKILL.md"),
            "missing frontmatter",
        )
        .unwrap();
        fs::write(source.join("mcp.json"), "{}").unwrap();
        assert_eq!(
            client
                .execute(client.prepare(install(worktree, 0)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadPlugin {
                    name: "example".into()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        fs::remove_dir_all(source.join("skills/invalid")).unwrap();
        let saved = info(execute(&client, install(worktree, 0)).await);
        assert_eq!(saved.skills.len(), 1);
        assert_eq!(saved.issues.len(), 1);
        assert_eq!(saved.issues[0].path, "mcp.json");
        assert_eq!(saved.issues[0].kind, plugin::IssueKind::InvalidMcp);
        for command in [
            Command::InstallPlugin {
                worktree: WorktreeId::new(),
                path: "package".into(),
                name: "example".into(),
                expected_revision: 1,
            },
            Command::InstallPlugin {
                worktree,
                path: "../node".into(),
                name: "example".into(),
                expected_revision: 1,
            },
            Command::InstallPlugin {
                worktree,
                path: "package".into(),
                name: "other".into(),
                expected_revision: 0,
            },
        ] {
            assert!(client.execute(client.prepare(command)).await.is_err());
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
            saved
        );
        let mut wrong_target = client.prepare(install(worktree, 1));
        wrong_target.target = NodeId([99; 32]);
        assert_eq!(
            client.execute(wrong_target).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        let read = client.prepare(Command::ListPlugins);
        let pending = client.dispatch(read).await.unwrap();
        assert!(!pending.receipt.durable);
        pending.completion.await.unwrap().unwrap();
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
