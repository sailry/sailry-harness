use super::*;

#[tokio::test]
async fn projects_node_owned_declarations() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let mut settings = read(&client).await;
        assert_eq!(
            settings.capabilities[&Driver::Exa].quota,
            search::Quota::AnonymousQuota
        );
        settings
            .capabilities
            .get_mut(&Driver::Tinyfish)
            .unwrap()
            .key_required = false;
        let saved = save(
            &client,
            settings,
            BTreeMap::from([(
                Driver::Exa,
                search::Credentials {
                    key: SecretUpdate::Replace(Secret::new("quota-fixture-key".into())),
                    ..Default::default()
                },
            )]),
        )
        .await;
        assert_eq!(
            saved.capabilities[&Driver::Exa].quota,
            search::Quota::FreeQuota
        );
        assert!(saved.capabilities[&Driver::Tinyfish].key_required);
        assert_eq!(read(&client).await, saved);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
