use super::*;
use persistence::{read, save, state};
use plugin::settings::SecretUpdate;

#[tokio::test]
async fn binds_protected_slots_to_real_mcp_targets() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        declare(&root, "Settings");
        let installed = info(execute(&client, install(worktree, 0)).await);
        let first = state(
            execute(
                &client,
                save(
                    &installed.summary.reference(),
                    "Original",
                    SecretUpdate::Replace(Secret::new("private-token".into())),
                ),
            )
            .await,
        );
        assert!(first.ready);
        // Ordinary immutable code updates retain the actual unchanged destination.
        fs::write(root.join("content.txt"), "New package content").unwrap();
        let current = inventory(&client).await.remove(0);
        let second = info(execute(&client, install(worktree, current.revision)).await);
        let unchanged = read(&client, &second.summary.reference()).await;
        assert!(unchanged.ready);
        assert_eq!(unchanged.configured, first.configured);
        let mut mcp: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("mcp.json")).unwrap()).unwrap();
        mcp["mcpServers"]["service"]["command"] = json!("changed-program");
        fs::write(root.join("mcp.json"), mcp.to_string()).unwrap();
        let changed = info(execute(&client, install(worktree, second.summary.revision)).await);
        let unbound = read(&client, &changed.summary.reference()).await;
        assert!(!unbound.ready);
        assert!(unbound.configured.is_empty());
        assert!(unbound.keepable.is_empty());
        assert_eq!(
            client
                .execute(client.prepare(save(&unbound.package, "Changed", SecretUpdate::Keep)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let replaced = state(
            execute(
                &client,
                save(
                    &unbound.package,
                    "Changed",
                    SecretUpdate::Replace(Secret::new("new-token".into())),
                ),
            )
            .await,
        );
        assert!(replaced.ready);
        let cleared = state(
            execute(
                &client,
                save(&replaced.package, "Changed", SecretUpdate::Clear),
            )
            .await,
        );
        assert!(cleared.configured.is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
