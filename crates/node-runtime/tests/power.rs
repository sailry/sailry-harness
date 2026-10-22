use sailry_client::Client;
use sailry_link::{Link, NetworkScope, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{power::Settings, *};
use std::sync::Arc;

#[tokio::test]
async fn authority_survives_replay_and_restart() {
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
    let transports: [Arc<dyn Transport>; 2] =
        [node.local(), controller.handle().remote(address.clone())];
    let mut expected = Settings::default();
    assert!(node.power_status().enabled);
    if node.power_status().supported && node.power_status().error.is_none() {
        assert!(node.power_status().acquired);
    }
    for transport in transports {
        let client = Client::new(transport);
        assert_eq!(read(&client).await, expected);
        let mut updates = client.subscribe().await.unwrap();
        let _ = updates.next().await.unwrap();
        let mut disable = expected.clone();
        disable.prevent_sleep = false;
        let old = client.prepare(Command::SavePowerSettings(disable.clone()));
        assert!(old.command.durable());
        let first = client.execute(old.clone()).await.unwrap();
        let Output::PowerSettings(mut updated) = first.clone() else {
            panic!("settings expected")
        };
        assert!(!node.power_status().enabled);
        assert!(!node.power_status().acquired);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Update::Event(envelope) = updates.next().await.unwrap()
                    && let Event::PowerSettingsChanged { revision } = envelope.event
                {
                    assert_eq!(revision, updated.revision);
                    break;
                }
            }
        })
        .await
        .unwrap();
        updated.prevent_sleep = true;
        let Output::PowerSettings(enabled) = client
            .execute(client.prepare(Command::SavePowerSettings(updated)))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        assert!(node.power_status().enabled);
        if node.power_status().supported && node.power_status().error.is_none() {
            assert!(node.power_status().acquired);
        }
        // The original completion remains stable without reapplying its old policy.
        assert_eq!(client.execute(old).await.unwrap(), first);
        assert_eq!(read(&client).await, enabled);
        assert!(node.power_status().enabled);
        assert_eq!(
            client
                .execute(client.prepare(Command::SavePowerSettings(disable)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.power_settings_revision, enabled.revision);
        let mut disable = enabled;
        disable.prevent_sleep = false;
        let Output::PowerSettings(saved) = client
            .execute(client.prepare(Command::SavePowerSettings(disable)))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        expected = saved;
    }
    let local = Client::new(node.local());
    expected.prevent_sleep = true;
    let Output::PowerSettings(enabled) = local
        .execute(local.prepare(Command::SavePowerSettings(expected)))
        .await
        .unwrap()
    else {
        panic!("settings expected")
    };
    controller.close().await.unwrap();
    assert!(node.power_status().enabled);
    if node.power_status().supported && node.power_status().error.is_none() {
        assert!(node.power_status().acquired);
    }
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let reconnected = Client::new(controller.handle().remote(address));
    assert_eq!(read(&reconnected).await, enabled);
    let mut disabled = enabled;
    disabled.prevent_sleep = false;
    let Output::PowerSettings(saved) = reconnected
        .execute(reconnected.prepare(Command::SavePowerSettings(disabled)))
        .await
        .unwrap()
    else {
        panic!("settings expected")
    };
    expected = saved;
    controller.close().await.unwrap();
    assert!(!node.power_status().enabled);
    assert!(!node.power_status().acquired);
    node.shutdown().await.unwrap();
    let node = Node::start_empty(&profile).await.unwrap();
    assert_eq!(read(&Client::new(node.local())).await, expected);
    assert!(!node.power_status().enabled);
    node.shutdown().await.unwrap();
}

async fn read(client: &Client) -> Settings {
    let request = client.prepare(Command::ReadPowerSettings);
    assert!(!request.command.durable());
    let Output::PowerSettings(settings) = client.execute(request).await.unwrap() else {
        panic!("settings expected")
    };
    settings
}

#[tokio::test]
async fn adds_missing_policy_without_resetting_data() {
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
    let path = profile.join("storage/node.sqlite3");
    let database = rusqlite::Connection::open(&path).unwrap();
    database.execute("DROP TABLE power_settings", []).unwrap();
    drop(database);
    let node = Node::start_empty(&profile).await.unwrap();
    assert_eq!(node.id(), identity);
    let client = Client::new(node.local());
    assert_eq!(read(&client).await, Settings::default());
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert_eq!(snapshot.projects.len(), 1);
    assert_eq!(snapshot.projects[0].name, "Preserved project");
    node.shutdown().await.unwrap();
    let database = rusqlite::Connection::open(&path).unwrap();
    let invalid = br#"{"revision":1,"prevent_sleep":"invalid"}"#;
    database
        .execute(
            "INSERT INTO power_settings(singleton,body) VALUES(1,?1)",
            [invalid.as_slice()],
        )
        .unwrap();
    drop(database);
    assert!(Node::start_empty(&profile).await.is_err());
    let database = rusqlite::Connection::open(path).unwrap();
    let preserved: Vec<u8> = database
        .query_row(
            "SELECT body FROM power_settings WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preserved, invalid);
}
