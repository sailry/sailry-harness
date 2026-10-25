use super::*;
use serde_json::json;
use turso::params;

#[tokio::test]
async fn rejects_retired_current_declarations() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let manifest = directory.path().join("source/package/plugin.json");
        let mut source: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        let mut revision = 0;
        for extension in [
            json!({"api_version":"v1","actions":["code.index.read"],"enabled_service":"language"}),
            json!({"api_version":"v1","actions":["code.read"]}),
            json!({"api_version":"v1","actions":["code.index.read"],
                "tools":[{"name":"legacy_symbols","operation":"code.symbols"}]}),
        ] {
            source["extensions"] = json!({"dev.sailry.platform":extension});
            fs::write(&manifest, source.to_string()).unwrap();
            let installed = info(execute(&client, install(worktree, revision)).await);
            revision = installed.summary.revision;
            assert!(installed.extension.is_none());
            assert!(
                installed
                    .issues
                    .iter()
                    .any(|issue| issue.kind == plugin::IssueKind::InvalidExtensions)
            );
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn reads_retired_metadata_after_reopen() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let (session, turn) = super::refresh::capture(&client, worktree).await;
        let legacy: Info =
            serde_json::from_str(include_str!("../../fixtures/legacy-code.json")).unwrap();
        let body = serde_json::to_vec(&legacy).unwrap();
        let profile = directory.path().join("node");
        node.shutdown().await.unwrap();
        let (handle, mut db) =
            crate::plugins::database_fixture::open(&profile.join("storage/node.turso"), false)
                .await;
        let transaction = db.transaction().await.unwrap();
        // Seed an isolated pre-upgrade capture; no live package installation accepts this declaration.
        transaction
            .execute(
                "INSERT INTO plugin_packages(digest,name,body) VALUES(?1,?2,?3)",
                params![
                    legacy.summary.digest.as_str(),
                    legacy.summary.name.as_str(),
                    body.clone()
                ],
            )
            .await
            .unwrap();
        let mut packages: Vec<plugin::Reference> = serde_json::from_slice(
            &transaction
                .prepare("SELECT plugins FROM turns WHERE id=?1")
                .await
                .unwrap()
                .query_row([turn.to_string()])
                .await
                .unwrap()
                .get::<Vec<u8>>(0)
                .unwrap(),
        )
        .unwrap();
        let captured = packages
            .iter_mut()
            .find(|package| package.name == "code")
            .unwrap();
        *captured = legacy.summary.reference();
        let capture = serde_json::to_vec(&packages).unwrap();
        transaction
            .execute(
                "UPDATE turns SET plugins=?1 WHERE id=?2",
                params![capture.clone(), turn.to_string()],
            )
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        crate::plugins::database_fixture::close(handle, db).await;
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let retained = info(
            execute(
                &client,
                Command::ReadPluginVersion {
                    package: legacy.summary.reference(),
                },
            )
            .await,
        );
        assert_eq!(retained, legacy);
        let Output::QueuedTurn(queued) = execute(
            &client,
            Command::ReadTurnCapture {
                session: session.id,
                turn,
            },
        )
        .await
        else {
            panic!("turn capture expected");
        };
        assert_eq!(queued.plugins, packages);
        let digest = legacy.summary.digest.clone();
        let stored: Vec<u8> = node
            .with_storage(move |db| {
                Box::pin(async move {
                    db.prepare("SELECT body FROM plugin_packages WHERE digest=?1")
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?
                        .query_row([digest])
                        .await
                        .map_err(crate::plugins::database_fixture::fault)?
                        .get(0)
                        .map_err(crate::plugins::database_fixture::fault)
                })
            })
            .await
            .unwrap();
        assert_eq!(stored, body);
        assert_eq!(
            retained.extension.unwrap().enabled_service,
            Some(plugin::Service::Language)
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
