use super::*;
use serde_json::json;

#[tokio::test]
async fn holds_waits_across_controller_disconnect() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let node = Node::start_empty(fixture.path().join("node"))
        .await
        .unwrap();
    let controller = Link::controller(fixture.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address.clone()));
    let server = Server::tools(vec![(
        plugin_tool("files", "write_file"),
        json!({"path":"pending.txt","text":"pending","expected_revision":null}),
    )])
    .await;
    let session = approvals::prepare_files(&remote, &server, &root).await;
    assert_eq!(node.power_status().active, 0);
    let turn = approvals::submit(&remote, session.id).await;
    approvals::pending(&remote, session.id).await;
    assert_eq!(node.power_status().active, 1);
    controller.close().await.unwrap();
    assert_eq!(node.power_status().active, 1);
    let local = Client::new(node.local());
    let Output::PowerSettings(mut settings) = local
        .execute(local.prepare(Command::ReadPowerSettings))
        .await
        .unwrap()
    else {
        panic!("settings expected")
    };
    settings.prevent_sleep = false;
    let Output::PowerSettings(mut settings) = local
        .execute(local.prepare(Command::SavePowerSettings(settings)))
        .await
        .unwrap()
    else {
        panic!("settings expected")
    };
    assert_eq!(node.power_status().active, 1);
    assert!(!node.power_status().acquired);
    settings.prevent_sleep = true;
    local
        .execute(local.prepare(Command::SavePowerSettings(settings)))
        .await
        .unwrap();
    assert!(node.power_status().enabled);
    let controller = Link::controller(fixture.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let reconnected = Client::new(controller.handle().remote(address));
    let Output::PowerSettings(settings) = reconnected
        .execute(reconnected.prepare(Command::ReadPowerSettings))
        .await
        .unwrap()
    else {
        panic!("settings expected")
    };
    assert!(settings.prevent_sleep);
    assert_eq!(settings.revision, 2);
    assert_eq!(node.power_status().active, 1);
    local
        .execute(local.prepare(Command::StopTurn { turn }))
        .await
        .unwrap();
    finished(&local, session.id, turn).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while node.power_status().active != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!root.join("pending.txt").exists());
    assert!(!node.power_status().acquired);
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}
