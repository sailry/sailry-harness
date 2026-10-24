use super::*;

async fn values(db: &turso::Connection) -> (i64, i64, i64) {
    let row = db.prepare("SELECT (SELECT count(*) FROM plugin_packages),(SELECT count(*) FROM plugin_settings),(SELECT count(*) FROM plugin_settings,json_each(plugin_settings.body,'$.slots') WHERE json_each.type='text')").await.unwrap().query_row(()).await.unwrap();
    (
        row.get(0).unwrap(),
        row.get(1).unwrap(),
        row.get(2).unwrap(),
    )
}

async fn counts(node: &Node) -> (i64, i64, i64) {
    node.with_storage(|db| Box::pin(async move { Ok(values(db).await) }))
        .await
        .unwrap()
}

async fn closed(profile: &Path) -> (i64, i64, i64) {
    let (handle, db) =
        crate::database_fixture::open(&profile.join("storage/node.turso"), true).await;
    let result = values(&db).await;
    crate::database_fixture::close(handle, db).await;
    result
}

#[tokio::test]
async fn keeps_frozen_resources() {
    for remote in [false, true] {
        let model = Server::tools(vec![(alias("native", "read"), json!({}))]).await;
        let mut fixture = process::Fixture::packages(remote, &model, &[]).await;
        isolate(&fixture.client).await;
        let profile = fixture.node.profile().to_owned();
        let baseline = counts(&fixture.node).await;
        let Output::Plugins(initial) = execute(&fixture.client, Command::ListPlugins).await else {
            panic!("plugin inventory expected")
        };
        declare(&fixture.root);
        let installed = install(&fixture, 0).await;
        let secret = "retained-fixture-token";
        let first = save(
            &fixture,
            installed.summary.reference(),
            "Retained",
            SecretUpdate::Replace(Secret::new(secret.into())),
        )
        .await;

        let (_, queued) = submit(&fixture, true).await;
        save(
            &fixture,
            first.package.clone(),
            "Unused",
            SecretUpdate::Replace(Secret::new("unused-fixture-token".into())),
        )
        .await;
        let Output::Plugin(current) = execute(
            &fixture.client,
            Command::ReadPlugin {
                name: "example".into(),
            },
        )
        .await
        else {
            panic!("plugin expected")
        };
        fs::write(fixture.root.join("package/version.txt"), "unused version").unwrap();
        let unused = install(&fixture, current.summary.revision).await;
        execute(
            &fixture.client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: unused.summary.revision,
            },
        )
        .await;
        let packages = profile.join("plugins/packages");
        assert_eq!(
            counts(&fixture.node).await,
            (baseline.0 + 2, baseline.1 + 2, baseline.2 + 2)
        );
        fixture.node.shutdown().await.unwrap();
        assert_eq!(
            closed(&profile).await,
            (baseline.0 + 1, baseline.1 + 1, baseline.2 + 1)
        );
        assert!(packages.join(&first.package.digest).is_dir());
        assert!(!packages.join(&unused.summary.digest).exists());
        fixture.node = Node::start_empty(&profile).await.unwrap();
        fixture.client = Client::new(if remote {
            fixture
                .controller
                .handle()
                .remote(fixture.node.link().address())
        } else {
            fixture.node.local()
        });
        let Output::Plugins(plugins) = execute(&fixture.client, Command::ListPlugins).await else {
            panic!("plugin inventory expected")
        };
        assert_eq!(
            plugins, initial,
            "removed packages must not reappear after restart"
        );

        let Output::QueuedMessage(restored) =
            execute(&fixture.client, Command::ReadQueuedTurn { turn: queued.id }).await
        else {
            panic!("queued message expected")
        };
        assert_eq!(restored.turn.plugins, queued.plugins);
        assert!(restored.turn.plugins.contains(&first.package));
        execute(
            &fixture.client,
            Command::StartQueuedTurn { turn: queued.id },
        )
        .await;
        let queue = history(&fixture.client, fixture.session.id).await.queue;
        assert!(queue.paused);
        assert!(
            queue
                .items
                .iter()
                .any(|item| item.turn == queued.id && item.ready)
        );
        execute(
            &fixture.client,
            Command::SetQueuePaused {
                session: fixture.session.id,
                expected_revision: queue.revision,
                paused: false,
            },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        verify(&page, "Retained", secret);
        // Completed history still owns the frozen revision, even without a live catalog entry.
        fixture.node.shutdown().await.unwrap();
        reaped(&profile.join("plugins/data/example"));
        assert_eq!(
            closed(&profile).await,
            (baseline.0 + 1, baseline.1 + 1, baseline.2 + 1)
        );
        fixture.controller.close().await.unwrap();
    }
}
