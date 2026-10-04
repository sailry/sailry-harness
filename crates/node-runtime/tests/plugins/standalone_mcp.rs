use super::*;
use plugin::{mcp::Definition, settings::SecretUpdate};
use std::collections::BTreeMap;

fn command(revision: u64, endpoint: &str, update: SecretUpdate) -> Command {
    Command::InstallMcp {
        name: "mcp-reporting".into(),
        expected_revision: revision,
        definition: Definition::StreamableHttp {
            url: endpoint.into(),
            headers: vec!["Authorization".into()],
        },
        secrets: BTreeMap::from([("Authorization".into(), update)]),
    }
}

async fn settings(client: &Client, info: &Info) -> plugin::settings::State {
    let Output::PluginSettings(state) = execute(
        client,
        Command::ReadPluginSettings {
            package: info.summary.reference(),
        },
    )
    .await
    else {
        panic!("MCP settings expected")
    };
    state
}

async fn start(remote: bool) -> (tempfile::TempDir, Node, Link, Client) {
    let directory = tempfile::tempdir().unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
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
    (directory, node, controller, client)
}

#[tokio::test]
async fn manages_without_projects() {
    for remote in [false, true] {
        let (directory, node, controller, client) = start(remote).await;
        let secret = "standalone-mcp-private-${PLUGIN_ROOT}-token";
        let request = client.prepare(command(
            0,
            "https://example.com/mcp",
            SecretUpdate::Replace(Secret::new(secret.into())),
        ));
        let mut subscription = client.subscribe().await.unwrap();
        subscription.next().await.unwrap();
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
        let event = subscription.next().await.unwrap();
        assert!(!serde_json::to_string(&event).unwrap().contains(secret));
        assert!(first.mcp_source.is_some());
        assert_eq!(first.mcp.len(), 1);
        assert!(first.skills.is_empty());
        let configured = settings(&client, &first).await;
        assert!(configured.ready);
        assert_eq!(configured.configured.len(), 1);
        assert!(configured.values.is_empty());
        assert_eq!(info(client.execute(request.clone()).await.unwrap()), first);
        let mut changed = request.clone();
        changed.command = command(0, "https://example.com/mcp", SecretUpdate::Clear);
        assert_eq!(
            client.execute(changed).await.unwrap_err().code,
            ErrorCode::Conflict
        );

        let updated = info(
            execute(
                &client,
                command(
                    first.summary.revision,
                    "https://example.com/updated",
                    SecretUpdate::Keep,
                ),
            )
            .await,
        );
        assert_ne!(updated.summary.digest, first.summary.digest);
        assert_eq!(
            settings(&client, &updated).await.configured,
            configured.configured
        );
        assert_eq!(
            client
                .execute(client.prepare(command(
                    first.summary.revision,
                    "https://example.com/stale",
                    SecretUpdate::Clear,
                )))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: updated.summary.name.clone(),
                    expected_revision: updated.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        assert!(!disabled.summary.enabled);
        let snapshot = execute(&client, Command::Snapshot).await;
        let Output::Snapshot(view) = &snapshot else {
            panic!("snapshot expected")
        };
        assert!(view.projects.is_empty() && view.worktrees.is_empty());
        for public in [
            serde_json::to_string(&snapshot).unwrap(),
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&configured).unwrap(),
        ] {
            assert!(!public.contains(secret));
        }
        let root = node
            .profile()
            .join("plugins/packages")
            .join(&first.summary.digest);
        for file in [
            "plugin.json",
            "mcp.json",
            "dev.sailry.platform/settings.json",
        ] {
            assert!(
                !fs::read_to_string(root.join(file))
                    .unwrap()
                    .contains(secret)
            );
        }
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
        assert!(body.starts_with(b"credential-v1:"));
        assert!(!result.contains(secret));
        drop(db);
        drop(subscription);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            inventory(&client).await.as_slice(),
            std::slice::from_ref(&disabled.summary)
        );
        assert_eq!(info(client.execute(request).await.unwrap()), first);
        assert_eq!(
            settings(&client, &disabled).await.configured,
            configured.configured
        );
        let enabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: disabled.summary.name,
                    expected_revision: disabled.summary.revision,
                    enabled: true,
                },
            )
            .await,
        );
        assert!(enabled.summary.enabled);
        let cleared = info(
            execute(
                &client,
                command(
                    enabled.summary.revision,
                    "https://example.com/updated",
                    SecretUpdate::Clear,
                ),
            )
            .await,
        );
        assert!(settings(&client, &cleared).await.configured.is_empty());
        execute(
            &client,
            Command::RemovePlugin {
                name: cleared.summary.name,
                expected_revision: cleared.summary.revision,
            },
        )
        .await;
        assert!(inventory(&client).await.is_empty());
        drop(client);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rolls_back_invalid_credentials() {
    for remote in [false, true] {
        let (_directory, node, controller, client) = start(remote).await;
        let invalid = command(
            0,
            "https://example.com/mcp",
            SecretUpdate::Replace(Secret::new("".into())),
        );
        assert_eq!(
            client
                .execute(client.prepare(invalid))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert!(inventory(&client).await.is_empty());
        let first = info(
            execute(
                &client,
                command(
                    0,
                    "https://example.com/mcp",
                    SecretUpdate::Replace(Secret::new("original-private-token".into())),
                ),
            )
            .await,
        );
        let invalid = Command::InstallMcp {
            name: first.summary.name.clone(),
            expected_revision: first.summary.revision,
            definition: Definition::StreamableHttp {
                url: "https://example.com/invalid-edit".into(),
                headers: vec!["Authorization".into(), "X-Token".into()],
            },
            secrets: BTreeMap::from([("Authorization".into(), SecretUpdate::Keep)]),
        };
        assert_eq!(
            client
                .execute(client.prepare(invalid))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            inventory(&client).await.as_slice(),
            std::slice::from_ref(&first.summary)
        );
        assert_eq!(settings(&client, &first).await.configured.len(), 1);
        drop(client);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
