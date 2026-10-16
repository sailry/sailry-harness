use super::*;

#[tokio::test]
async fn manages_catalog_without_projects() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let bin = directory.path().canonicalize().unwrap().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let node = Node::start_empty_with_environment(
            directory.path().join("node"),
            environment(directory.path(), &bin),
        )
        .await
        .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport.clone());
        let mut updates = transport
            .subscribe(sailry_protocol::Topic::CodeInstallations)
            .await
            .unwrap();
        assert!(
            matches!(updates.next().await.unwrap(), sailry_protocol::Update::CodeInstallations { node: target, installations } if target == node.id() && installations.is_empty())
        );
        let Output::CodeCatalog(catalog) = client
            .execute(client.prepare(Command::ReadCodeCatalog))
            .await
            .unwrap()
        else {
            panic!("catalog expected")
        };
        for name in ["rust", "html", "python", "javascript"] {
            assert!(catalog.iter().any(|language| language.name == name));
        }
        assert!(
            catalog
                .iter()
                .flat_map(|language| &language.servers)
                .all(|server| server.startup.is_none()
                    && server.status == code::Status::NotInstalled)
        );
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert!(snapshot.projects.is_empty());
        assert!(snapshot.worktrees.is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
