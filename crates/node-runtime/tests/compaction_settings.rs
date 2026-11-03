use sailry_client::Client;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{conversation::compaction::Settings, *};
use std::{sync::Arc, time::Duration};

async fn read(client: &Client) -> Settings {
    let Output::CompactionSettings(settings) = client
        .execute(client.prepare(Command::ReadCompactionSettings))
        .await
        .unwrap()
    else {
        panic!("compaction settings expected")
    };
    settings
}

#[tokio::test]
async fn saves_replays_and_restores_on_both_transports() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let node = Node::start_empty(&profile).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let transports: [Arc<dyn Transport>; 2] = [node.local(), controller.handle().remote(address)];
    let mut expected = Settings::default();
    for transport in transports {
        let client = Client::new(transport);
        assert_eq!(read(&client).await, expected);
        let mut events = client.subscribe().await.unwrap();
        events.next().await.unwrap();
        let request = client.prepare(Command::SaveCompactionSettings(Settings {
            percent: 65,
            ..expected
        }));
        let first = client.execute(request.clone()).await.unwrap();
        let Output::CompactionSettings(saved) = first.clone() else {
            panic!("settings expected")
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Update::Event(envelope) = events.next().await.unwrap()
                    && let Event::CompactionSettingsChanged { revision } = envelope.event
                {
                    assert_eq!(revision, saved.revision);
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(
            client
                .execute(client.prepare(Command::SaveCompactionSettings(expected)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        for percent in [0, 100] {
            assert_eq!(
                client
                    .execute(client.prepare(Command::SaveCompactionSettings(Settings {
                        percent,
                        ..saved
                    })))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        let Output::CompactionSettings(updated) = client
            .execute(client.prepare(Command::SaveCompactionSettings(Settings {
                percent: 90,
                ..saved
            })))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        assert_eq!(client.execute(request).await.unwrap(), first);
        assert_eq!(read(&client).await, updated);
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.compaction_settings_revision, updated.revision);
        expected = updated;
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start_empty(&profile).await.unwrap();
    assert_eq!(read(&Client::new(node.local())).await, expected);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn upgrades_without_replacing_existing_data() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let node = Node::start_empty(&profile).await.unwrap();
    let identity = node.id();
    let client = Client::new(node.local());
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Preserved project".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    node.shutdown().await.unwrap();
    let path = profile.join("storage/node.turso");
    let (database, db) = crate::database_fixture::open(&path, false).await;
    db.execute("DROP TABLE compaction_settings", ())
        .await
        .unwrap();
    db.execute("ALTER TABLE agent_runs DROP COLUMN compaction_percent", ())
        .await
        .unwrap();
    crate::database_fixture::close(database, db).await;
    let node = Node::start_empty(&profile).await.unwrap();
    assert_eq!(node.id(), identity);
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    for transport in [node.local(), controller.handle().remote(address)] {
        let client = Client::new(transport);
        assert_eq!(read(&client).await, Settings::default());
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.projects[0].name, "Preserved project");
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let (database, db) = crate::database_fixture::open(&path, false).await;
    let invalid = br#"{"revision":1,"percent":100}"#;
    db.execute(
        "INSERT INTO compaction_settings(singleton,body) VALUES(1,?1)",
        [invalid.as_slice()],
    )
    .await
    .unwrap();
    crate::database_fixture::close(database, db).await;
    assert!(Node::start_empty(&profile).await.is_err());
    let (database, db) = crate::database_fixture::open(&path, true).await;
    let body: Vec<u8> = db
        .prepare("SELECT body FROM compaction_settings WHERE singleton=1")
        .await
        .unwrap()
        .query_row(())
        .await
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(body, invalid);
    crate::database_fixture::close(database, db).await;
}
