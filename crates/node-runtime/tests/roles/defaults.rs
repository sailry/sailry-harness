use super::*;

fn shipped(profiles: &[role::Profile]) -> &role::Profile {
    assert_eq!(profiles.len(), 2);
    let role = &profiles[0];
    assert_eq!(role.key, "explore");
    assert_eq!(role.name, "Explore");
    assert_eq!(role.revision, 1);
    assert!(role.read_only);
    assert!(role.model.is_none());
    assert!(role.skills.is_empty());
    assert!(role.instructions.contains("read-only tools"));
    role.validate().unwrap();
    let research = &profiles[1];
    assert_eq!(research.key, "research");
    assert_eq!(research.name, "Research");
    assert_eq!(research.revision, 1);
    assert!(research.read_only);
    assert!(research.model.is_none());
    assert_eq!(research.max_turns, Some(8));
    assert_eq!(research.skills, ["web-search:research"]);
    assert!(research.instructions.contains("inherited permissions"));
    research.validate().unwrap();
    role
}

#[tokio::test]
async fn preserves_edits_and_removal() {
    for remote in [false, true] {
        let (directory, node, controller, client) = fresh(remote).await;
        let defaults = list(&client).await;
        let original = shipped(&defaults).clone();
        let research = defaults[1].clone();
        let mut changed = original.clone();
        changed.key = "custom-explorer".into();
        changed.name = "Custom explorer".into();
        changed.read_only = false;
        changed.instructions = "Keep the user's edited role instructions".into();
        let changed = put(&client, changed, original.revision).await.unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(list(&client).await, [changed.clone(), research.clone()]);
        client
            .execute(client.prepare(Command::RemoveRole {
                role: changed.id,
                expected_revision: changed.revision,
            }))
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::RemoveRole {
                role: research.id,
                expected_revision: research.revision,
            }))
            .await
            .unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert!(list(&client).await.is_empty());
        let other = Node::start(directory.path().join("other")).await.unwrap();
        let other_role = shipped(&list(&Client::new(other.local())).await).clone();
        assert_ne!(other_role.id, original.id);
        other.shutdown().await.unwrap();
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn refreshes_only_pristine_explore_instructions() {
    const PREVIOUS: &str = "Explore the assigned scope using read-only tools. Read existing code and relevant project instructions, distinguish confirmed facts from inferences, and return concise findings with exact paths and unresolved questions. Do not modify resources or request user input.";
    for remote in [false, true] {
        for customized in [false, true] {
            let (directory, node, controller, client) = fresh(remote).await;
            let profiles = list(&client).await;
            let original = profiles[0].clone();
            let mut previous = original.clone();
            previous.instructions = PREVIOUS.into();
            if customized {
                previous.name = "Custom explorer".into();
                previous.revision += 1;
            }
            node.shutdown().await.unwrap();
            let path = directory.path().join("node/storage/node.turso");
            let (database, connection) = database_fixture::open(&path, false).await;
            connection
                .execute(
                    "UPDATE roles SET body=?1 WHERE id=?2",
                    turso::params![
                        serde_json::to_vec(&previous).unwrap(),
                        previous.id.to_string()
                    ],
                )
                .await
                .unwrap();
            database_fixture::close(database, connection).await;
            let node = Node::start(directory.path().join("node")).await.unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            let current = list(&client).await;
            assert_eq!(current[1], profiles[1]);
            if customized {
                assert_eq!(current[0], previous);
            } else {
                let mut expected = original;
                expected.revision += 1;
                assert_eq!(current[0], expected);
                let backups = std::fs::read_dir(directory.path().join("node/storage/backups"))
                    .unwrap()
                    .map(|entry| entry.unwrap().path().join("node.turso"))
                    .collect::<Vec<_>>();
                assert_eq!(backups.len(), 1);
                let (database, connection) = database_fixture::open(&backups[0], true).await;
                let saved = connection
                    .prepare("SELECT body FROM roles WHERE id=?1")
                    .await
                    .unwrap()
                    .query_row([previous.id.to_string()])
                    .await
                    .unwrap()
                    .get::<Vec<u8>>(0)
                    .unwrap();
                assert_eq!(
                    serde_json::from_slice::<role::Profile>(&saved).unwrap(),
                    previous
                );
                database_fixture::close(database, connection).await;
            }
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
}
