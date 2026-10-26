use super::*;
use sailry_client::{Client, activity::Change};
use sailry_protocol::{
    Command, Output,
    notification::{Draft, Kind},
};
use std::{collections::BTreeMap, sync::Arc};

#[tokio::test]
async fn reads_notices_on_the_execution_node() {
    let directory = tempfile::tempdir().unwrap();
    let node = sailry_node_runtime::Node::start(directory.path().join("node"))
        .await
        .unwrap();
    let client = Arc::new(Client::new(node.local()));
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
    for remote in [false, true] {
        for _ in 0..3 {
            client
                .execute(client.prepare(Command::PublishNotification {
                    package: package.clone(),
                    content: Draft {
                        title: "Reminder".into(),
                        message: "Check the task".into(),
                        kind: Kind::Info,
                        session: None,
                        presentation: None,
                        details: None,
                    },
                }))
                .await
                .unwrap();
        }
        let state = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                updates.next().await.unwrap();
                let state: serde_json::Value =
                    serde_json::from_str(&controller.notifications().await.unwrap()).unwrap();
                if state["unread"] == 3 {
                    break state;
                }
            }
        })
        .await
        .unwrap();
        controller
            .mark_notification_read(state["notices"][0]["id"].to_string())
            .await
            .unwrap();
        let transport = if remote {
            Arc::new(Client::new(controller.handle.remote(node.link().address())))
        } else {
            client.clone()
        };
        let clients = BTreeMap::from([(transport.target(), transport)]);
        if remote {
            controller.mark_notifications_read().await.unwrap();
        } else {
            let batch = controller.inbox.lock().await.prepare(Change::ReadAll);
            let report = batch.run(&clients).await;
            assert_eq!(report.completed, 1);
            assert!(!report.failed());
            controller.inbox.lock().await.complete(&batch, &report);
        }
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.notifications.len(), 3);
        assert!(snapshot.notifications.iter().all(|notice| notice.read));
        assert_eq!(controller.inbox.lock().await.unread(), 0);

        let batch = controller.inbox.lock().await.prepare(Change::DismissRead);
        let report = batch.run(&clients).await;
        assert_eq!(report.completed, 1);
        assert!(!report.failed());
        controller.inbox.lock().await.complete(&batch, &report);
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert!(snapshot.notifications.is_empty());
    }
    updates.close();
    connection.close();
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
