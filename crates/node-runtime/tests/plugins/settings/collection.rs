use super::persistence::{read, save, state};
use super::*;
use plugin::settings::SecretUpdate;

#[path = "collection/media.rs"]
mod media;

async fn values(db: &turso::Connection) -> Result<(i64, i64, i64), Fault> {
    let row = db.prepare("SELECT (SELECT count(*) FROM plugin_packages),(SELECT count(*) FROM plugin_settings),(SELECT count(*) FROM plugin_settings,json_each(plugin_settings.body,'$.slots') WHERE json_each.type='text')")
            .await.map_err(crate::plugins::database_fixture::fault)?.query_row(()).await.map_err(crate::plugins::database_fixture::fault)?;
    Ok((
        row.get(0)
            .map_err(crate::plugins::database_fixture::fault)?,
        row.get(1)
            .map_err(crate::plugins::database_fixture::fault)?,
        row.get(2)
            .map_err(crate::plugins::database_fixture::fault)?,
    ))
}

async fn counts(node: &Node) -> (i64, i64, i64) {
    node.with_storage(|db| Box::pin(values(db))).await.unwrap()
}

async fn closed(profile: &Path) -> (i64, i64, i64) {
    let (handle, db) =
        crate::plugins::database_fixture::open(&profile.join("storage/node.turso"), true).await;
    let result = values(&db).await.unwrap();
    crate::plugins::database_fixture::close(handle, db).await;
    result
}

#[tokio::test]
async fn prunes_unused() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let profile = node.profile().to_owned();
        let Output::Plugins(defaults) = execute(&client, Command::ListPlugins).await else {
            panic!("plugin list expected")
        };
        let baseline = i64::try_from(defaults.len()).unwrap();
        assert_eq!(counts(&node).await, (baseline, 0, 0));
        let source = directory.path().join("source/package");
        declare(&source, "Settings");
        let installed = info(execute(&client, install(worktree, 0)).await);
        let request = client.prepare(save(
            &installed.summary.reference(),
            "https://first.example.com",
            SecretUpdate::Replace(Secret::new("first-fixture-secret".into())),
        ));
        let first = state(client.execute(request.clone()).await.unwrap());
        let second = state(
            execute(
                &client,
                save(
                    &first.package,
                    "https://second.example.com",
                    SecretUpdate::Replace(Secret::new("second-fixture-secret".into())),
                ),
            )
            .await,
        );
        let current = inventory(&client).await.remove(0);
        fs::write(source.join("version.txt"), "updated package").unwrap();
        let updated = info(execute(&client, install(worktree, current.revision)).await);
        assert_ne!(updated.summary.digest, installed.summary.digest);
        assert_eq!(
            updated.summary.settings_revision,
            second.package.settings_revision
        );
        let disabled = info(
            execute(
                &client,
                Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: updated.summary.revision,
                    enabled: false,
                },
            )
            .await,
        );
        let saved = read(&client, &disabled.summary.reference()).await;
        let packages = profile.join("plugins/packages");
        assert_eq!(counts(&node).await, (baseline + 2, 2, 2));
        node.shutdown().await.unwrap();
        assert_eq!(closed(&profile).await, (baseline + 1, 1, 1));
        assert!(!packages.join(&installed.summary.digest).exists());
        assert!(packages.join(&disabled.summary.digest).is_dir());
        assert!(source.join("plugin.json").exists());

        // An interrupted publication/deletion can leave a managed orphan without metadata.
        let orphan = packages.join(blake3::hash(b"unpublished fixture").to_hex().as_str());
        fs::create_dir(&orphan).unwrap();
        fs::write(orphan.join("content"), "unpublished fixture").unwrap();
        let node = Node::start(&profile).await.unwrap();
        assert!(!orphan.exists());
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(read(&client, &saved.package).await, saved);
        assert_eq!(state(client.execute(request.clone()).await.unwrap()), first);
        assert_eq!(
            counts(&node).await,
            (baseline + 1, 1, 1),
            "old receipts do not recreate collected settings"
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: disabled.summary.revision,
            },
        )
        .await;
        node.shutdown().await.unwrap();
        assert_eq!(closed(&profile).await, (baseline, 0, 0));
        assert!(!packages.join(&disabled.summary.digest).exists());

        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(state(client.execute(request).await.unwrap()), first);
        let reinstalled = info(execute(&client, install(worktree, 0)).await);
        assert!(reinstalled.summary.revision > disabled.summary.revision);
        assert_eq!(reinstalled.summary.settings_revision, 0);
        let third = state(
            execute(
                &client,
                save(
                    &reinstalled.summary.reference(),
                    "https://third.example.com",
                    SecretUpdate::Replace(Secret::new("third-fixture-secret".into())),
                ),
            )
            .await,
        );
        assert_eq!(third.package.settings_revision, 3);
        node.shutdown().await.unwrap();
        assert_eq!(closed(&profile).await, (baseline + 1, 1, 1));
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_unexpected_schema() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    node.shutdown().await.unwrap();
    let path = profile.join("storage/node.turso");
    let (db_handle, db) = crate::plugins::database_fixture::open(&path, false).await;
    db.execute_batch("ALTER TABLE plugins ADD COLUMN unexpected TEXT DEFAULT 'retained';")
        .await
        .unwrap();
    crate::plugins::database_fixture::close(db_handle, db).await;
    let before = fs::read(&path).unwrap();
    assert!(Node::start(&profile).await.is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[tokio::test]
async fn tolerates_cleanup_failure() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    node.shutdown().await.unwrap();
    let path = profile.join("plugins/packages");
    fs::rename(&path, profile.join("plugins/retained-packages")).unwrap();
    fs::write(&path, "unexpected file").unwrap();
    let node = Node::start(&profile).await.unwrap();
    assert!(inventory(&Client::new(node.local())).await.is_empty());
    node.shutdown().await.unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "unexpected file");
}
