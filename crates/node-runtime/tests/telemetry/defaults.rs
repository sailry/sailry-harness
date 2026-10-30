use super::*;

#[tokio::test]
async fn fills_missing_defaults_without_discarding_data() {
    for remote in [false, true] {
        for missing in ["table", "metrics", "identity"] {
            let directory = tempfile::tempdir().unwrap();
            let profile = directory.path().join("node");
            let root = directory.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let node = Node::start_empty(&profile).await.unwrap();
            let identity = node.id();
            let local = Client::new(node.local());
            let before = local.telemetry_settings().await.unwrap();
            let Output::Project(project) = local
                .execute(local.prepare(Command::RegisterProject {
                    name: "Preserved project".into(),
                    path: root.to_str().unwrap().into(),
                }))
                .await
                .unwrap()
            else {
                panic!("project expected");
            };
            let controller =
                Link::controller(directory.path().join("controller"), NetworkScope::default())
                    .await
                    .unwrap();
            controller
                .handle()
                .pair(node.link().invite().unwrap().ticket())
                .await
                .unwrap();
            node.shutdown().await.unwrap();
            let (database, connection) =
                crate::database_fixture::open(&profile.join("storage/node.turso"), false).await;
            if missing == "table" {
                connection
                    .execute("DROP TABLE telemetry_settings", ())
                    .await
                    .unwrap();
            } else {
                let value = if missing == "metrics" {
                    json!({"revision":9,"errors":false,"installation_id":before.installation_id})
                } else {
                    json!({"revision":9,"metrics":false})
                };
                connection
                    .execute(
                        "UPDATE telemetry_settings SET body=?1",
                        [serde_json::to_vec(&value).unwrap()],
                    )
                    .await
                    .unwrap();
            }
            crate::database_fixture::close(database, connection).await;
            let node = Node::start_empty(&profile).await.unwrap();
            assert_eq!(node.id(), identity);
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            let settings = client.telemetry_settings().await.unwrap();
            assert!(!settings.installation_id.is_nil());
            if missing == "table" {
                assert!(settings.errors);
                assert!(settings.metrics);
                assert_eq!(settings.revision, 0);
            } else if missing == "metrics" {
                assert!(!settings.errors);
                assert!(settings.metrics);
                assert_eq!(settings.installation_id, before.installation_id);
                assert_eq!(settings.revision, 9);
            } else {
                assert!(settings.errors);
                assert!(!settings.metrics);
                assert_ne!(settings.installation_id, before.installation_id);
                assert_eq!(settings.revision, 9);
            }
            assert!(profile.join("storage/backups").is_dir());
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected");
            };
            assert_eq!(snapshot.projects, vec![project]);
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
            let (database, connection) =
                crate::database_fixture::open(&profile.join("storage/node.turso"), true).await;
            let row = connection
                .prepare("SELECT body FROM telemetry_settings WHERE singleton=1")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap();
            let body: Vec<u8> = row.get(0).unwrap();
            assert_eq!(serde_json::from_slice::<Settings>(&body).unwrap(), settings);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body)
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .len(),
                4
            );
            crate::database_fixture::close(database, connection).await;
        }
    }
}
