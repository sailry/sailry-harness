use super::*;
use sailry_client::Client;
use sailry_protocol::{
    Command, Output,
    notification::{Draft, Kind},
};

#[tokio::test]
async fn reading_plugin_notice_updates_the_execution_node() {
    let directory = tempfile::tempdir().unwrap();
    let node = sailry_node_runtime::Node::start(directory.path().join("node"))
        .await
        .unwrap();
    let client = Client::new(node.local());
    let Output::Plugins(plugins) = client
        .execute(client.prepare(Command::ListPlugins))
        .await
        .unwrap()
    else {
        panic!("plugins expected")
    };
    let package = plugins
        .into_iter()
        .find(|plugin| plugin.name == "progress")
        .unwrap()
        .reference();
    client
        .execute(client.prepare(Command::PublishNotification {
            package,
            content: Draft {
                title: "Reminder".into(),
                message: "Check the task".into(),
                kind: Kind::Info,
                session: None,
            },
        }))
        .await
        .unwrap();
    let controller = Controller::open(
        directory.path().join("controller").to_str().unwrap().into(),
        false,
        vec![],
    )
    .await
    .unwrap();
    let address = controller
        .pair(node.link().invite().unwrap().ticket().into())
        .await
        .unwrap();
    let connection = controller.connect(address).unwrap();
    let updates = connection.watch().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            updates.next().await.unwrap();
            let state: serde_json::Value =
                serde_json::from_str(&controller.notifications().await.unwrap()).unwrap();
            if state["unread"] == 1 {
                controller
                    .mark_notification_read(state["notices"][0]["id"].to_string())
                    .await
                    .unwrap();
                break;
            }
        }
    })
    .await
    .unwrap();
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert!(snapshot.notifications[0].read);
    updates.close();
    connection.close();
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
