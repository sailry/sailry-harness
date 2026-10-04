use super::*;
use serde_json::json;
#[path = "settings/actions.rs"]
mod actions;
#[path = "settings/collection.rs"]
mod collection;
#[path = "settings/persistence.rs"]
mod persistence;
#[path = "settings/versions.rs"]
mod versions;

fn declare(root: &Path, title: &str) {
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("plugin.json")).unwrap()).unwrap();
    manifest["extensions"] = json!({"dev.sailry.platform":{
        "api_version":"v1", "actions":[], "settings_schema":"dev.sailry.platform/settings.json"
    }});
    fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
    fs::write(
        root.join("dev.sailry.platform/settings.json"),
        json!({
            "$schema":plugin::settings::SCHEMA, "type":"object", "additionalProperties":false,
            "title":title,
            "properties":{
                "endpoint":{"type":"string", "default":"https://example.com"},
                "token":{"type":"string", "x-sailry-secret":{"server":"service", "env":"TOKEN"}}
            },
            "required":["endpoint", "token"]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(root.join("mcp.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers":{"service":{"type":"stdio", "command":"never-run-during-install", "env":{"TOKEN":""}}}
    }).to_string()).unwrap();
}

#[tokio::test]
async fn recovers_versioned_forms() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        declare(&root, "First form");
        let request = client.prepare(install(worktree, 0));
        let first = info(client.execute(request.clone()).await.unwrap());
        let form = first.settings.as_ref().unwrap();
        assert_eq!(form.title.as_deref(), Some("First form"));
        assert_eq!(form.properties["token"].default, None);
        assert!(first.issues.is_empty());
        assert_eq!(first.skills.len(), 1);
        assert_eq!(first.mcp.len(), 1);
        declare(&root, "Changed form");
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
            first
        );
        let second = info(execute(&client, install(worktree, first.summary.revision)).await);
        assert_ne!(second.summary.digest, first.summary.digest);
        assert_eq!(
            second.settings.as_ref().unwrap().title.as_deref(),
            Some("Changed form")
        );
        assert_eq!(info(client.execute(request.clone()).await.unwrap()), first);
        node.shutdown().await.unwrap();
        let reopened = Node::start(directory.path().join("node")).await.unwrap();
        // Reconnect through the same authenticated path and persistent peer identity.
        let reconnected = Client::new(if remote {
            controller.handle().remote(reopened.link().address())
        } else {
            reopened.local()
        });
        assert_eq!(
            info(
                execute(
                    &reconnected,
                    Command::ReadPlugin {
                        name: "example".into()
                    }
                )
                .await
            ),
            second
        );
        assert_eq!(info(reconnected.execute(request).await.unwrap()), first);
        reopened.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn isolates_forms() {
    let first = fixture(false).await;
    let second = fixture(true).await;
    declare(&first.0.path().join("source/package"), "Host one");
    declare(&second.0.path().join("source/package"), "Host two");
    let one = info(execute(&first.3, install(first.4, 0)).await);
    let two = info(execute(&second.3, install(second.4, 0)).await);
    assert_ne!(one.summary.digest, two.summary.digest);
    assert_eq!(
        one.settings.as_ref().unwrap().title.as_deref(),
        Some("Host one")
    );
    assert_eq!(
        two.settings.as_ref().unwrap().title.as_deref(),
        Some("Host two")
    );
    fs::write(
        second
            .0
            .path()
            .join("source/package/dev.sailry.platform/settings.json"),
        json!({"$ref":"https://example.com/do-not-fetch"}).to_string(),
    )
    .unwrap();
    let invalid = info(execute(&second.3, install(second.4, two.summary.revision)).await);
    assert!(invalid.settings.is_none());
    assert!(
        invalid
            .issues
            .iter()
            .any(|issue| issue.path == "dev.sailry.platform/settings.json"
                && issue.kind == plugin::IssueKind::InvalidExtensions)
    );
    assert_eq!(invalid.skills, two.skills);
    assert_eq!(invalid.mcp, two.mcp);
    assert_eq!(
        info(
            execute(
                &first.3,
                Command::ReadPlugin {
                    name: "example".into()
                }
            )
            .await
        ),
        one
    );
    first.1.shutdown().await.unwrap();
    second.1.shutdown().await.unwrap();
    first.2.close().await.unwrap();
    second.2.close().await.unwrap();
}
