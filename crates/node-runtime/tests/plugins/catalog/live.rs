use super::*;

#[tokio::test]
#[ignore = "requires the public official catalog and GitHub Git transport"]
async fn official_repository() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let (controller, client) = attach(remote, &directory, &node).await;
        let Output::PluginCatalog(page) = execute(&client, search("gomoku")).await else {
            panic!("official catalog expected");
        };
        assert_eq!(page.entries.len(), 1);
        let entry = &page.entries[0];
        assert_eq!(entry.name, "gomoku");
        assert!(
            !entry.bundled,
            "live index must not fall back to bundled packages"
        );
        let Output::PluginRepository(source) = execute(
            &client,
            Command::ReadCatalogPlugin {
                source: plugin::catalog::Source::Official,
                id: entry.id.clone(),
            },
        )
        .await
        else {
            panic!("repository expected");
        };
        let Output::PluginSource(selected) =
            execute(&client, Command::InspectPluginSource { source }).await
        else {
            panic!("immutable selection expected");
        };
        assert_eq!(
            selected.source.repository,
            "https://github.com/sailry/sailry-plugins.git"
        );
        assert_eq!(selected.source.commit.len(), 40);
        assert_eq!(selected.path, "gomoku");
        assert!(selected.info.issues.is_empty());
        let installed = info(
            execute(
                &client,
                Command::InstallPluginSource {
                    source: selected.source.clone(),
                    path: selected.path,
                    name: entry.name.clone(),
                    expected_revision: 0,
                },
            )
            .await,
        );
        assert_eq!(installed.summary.digest, selected.info.summary.digest);
        assert_eq!(installed.summary.name, "gomoku");
        assert!(matches!(
            installed.origin,
            Some(plugin::Origin::Online { .. })
        ));
        println!(
            "{} official catalog and pinned Git install passed at {}",
            if remote { "Remote" } else { "Local" },
            selected.source.commit
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
