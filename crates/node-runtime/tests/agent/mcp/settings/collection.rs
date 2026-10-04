use super::*;

fn counts(profile: &Path) -> (i64, i64, i64) {
    let db = rusqlite::Connection::open_with_flags(
        profile.join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    db.query_row("SELECT (SELECT count(*) FROM plugin_packages),(SELECT count(*) FROM plugin_settings),(SELECT count(*) FROM plugin_settings,json_each(plugin_settings.body,'$.slots') WHERE json_each.type='text')", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap()
}

#[tokio::test]
async fn keeps_frozen_resources() {
    for remote in [false, true] {
        let model = Server::tools(vec![(alias("native", "read"), json!({}))]).await;
        let mut fixture = process::Fixture::new(remote, &model).await;
        isolate(&fixture.client).await;
        let profile = fixture.node.profile().to_owned();
        let baseline = counts(&profile);
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
            counts(&profile),
            (baseline.0 + 2, baseline.1 + 2, baseline.2 + 2)
        );
        fixture.node.shutdown().await.unwrap();
        assert_eq!(
            counts(&profile),
            (baseline.0 + 1, baseline.1 + 1, baseline.2 + 1)
        );
        assert!(packages.join(&first.package.digest).is_dir());
        assert!(!packages.join(&unused.summary.digest).exists());
        fixture.node = Node::start(&profile).await.unwrap();
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
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        verify(&page, "Retained", secret);
        // Completed history still owns the frozen revision, even without a live catalog entry.
        fixture.node.shutdown().await.unwrap();
        reaped(&profile.join("plugins/data/example"));
        assert_eq!(
            counts(&profile),
            (baseline.0 + 1, baseline.1 + 1, baseline.2 + 1)
        );
        fixture.controller.close().await.unwrap();
    }
}
