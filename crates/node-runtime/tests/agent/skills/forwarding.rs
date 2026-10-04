use super::*;

#[tokio::test]
async fn uses_destination_inventory() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            "read_skill_resource".into(),
            json!({"skill": "example:analysis", "path": "references/guide.md"}),
        )])
        .await;
        let (directory, source, controller, origin, local) = fixture(false, &server).await;
        let target = Node::start(directory.path().join("target")).await.unwrap();
        source
            .link()
            .pair(target.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(target.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let destination = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            target.local()
        });
        let root = directory.path().join("target-project");
        package(&root, "2.0.0");
        let (existing, _) = configured(&destination, &server.endpoint, &root).await;
        isolate_model_tools(&destination).await;
        disable_tools(&destination, &["commands", "browser", "media"]).await;
        let Output::Snapshot(snapshot) = execute(&origin, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let provider = snapshot.providers[0].clone();
        execute(
            &origin,
            Command::SaveProvider {
                provider,
                expected_revision: 2,
                secret: Some(Secret::new("isolated-skill-fixture-secret".into())),
            },
        )
        .await;
        let Output::Snapshot(snapshot) = execute(&origin, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let provider = &snapshot.providers[0];
        let config = SessionConfig {
            assistant: None,
            resource: None,
            credential: provider.credential.clone(),
            ..local.config.clone()
        };
        let create = Command::CreateSessionAt {
            target: target.id(),
            project: existing.project,
            worktree: Some(existing.worktree),
            config: Box::new(config),
            provider_revision: provider.revision,
        };
        let request = origin.prepare(create);
        let Output::Session(imported) = origin.execute(request.clone()).await.unwrap() else {
            panic!("session expected")
        };
        assert_eq!(
            imported.config.credential.as_ref().unwrap().node,
            target.id()
        );
        assert_eq!(
            origin.execute(request).await.unwrap(),
            Output::Session(imported.clone())
        );
        drop(origin);
        source.shutdown().await.unwrap();
        package(&root, "2.0.0");
        let installed = install(&destination, existing.worktree, 0).await;
        enabled_only(&destination, &["example"]).await;
        let Output::QueuedTurn(turn) = execute(
            &destination,
            Command::SubmitTurn {
                session: imported.id,
                expected_revision: imported.revision,
                message: "Use the destination skill version".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        assert_eq!(turn.plugins, [installed.summary.reference()]);
        let page = finished(&destination, imported.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert_eq!(results(&page)[0]["content"], "Guide 2.0.0 中文 🙂");
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert!(
            server
                .authorization
                .lock()
                .unwrap()
                .iter()
                .all(|value| value.as_deref() == Some("Bearer isolated-skill-fixture-secret"))
        );
        drop(destination);
        controller.close().await.unwrap();
        target.shutdown().await.unwrap();
    }
}
