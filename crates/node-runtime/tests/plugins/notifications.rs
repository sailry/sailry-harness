use super::*;
use sailry_protocol::{
    notification::{Draft, Kind, Notice},
    plugin::Action,
};

fn draft() -> Draft {
    Draft {
        title: "Reminder".into(),
        message: "Review the task".into(),
        kind: Kind::Info,
        session: None,
    }
}

async fn notices(client: &Client) -> Vec<Notice> {
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    snapshot.notifications
}

#[tokio::test]
async fn durable_delivery_read_and_clear() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::Notify],
        )
        .await;
        let request = client
            .prepare(Command::PublishNotification {
                package: context.package.clone(),
                content: draft(),
            })
            .with_plugin(context);
        let output = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), output);
        let Output::Notification(notice) = output else {
            panic!("notification expected")
        };
        assert_eq!(notices(&client).await, vec![notice.clone()]);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(notices(&client).await, vec![notice.clone()]);
        let (sender, mut receiver) = tokio::sync::watch::channel(sailry_client::View::default());
        let stop = sailry_link::CancellationToken::new();
        let watch_client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let cancellation = stop.clone();
        let watching = tokio::spawn(async move { watch_client.watch(sender, cancellation).await });
        tokio::time::timeout(Duration::from_secs(10), async {
            while receiver.borrow().notifications.is_empty() {
                receiver.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let view = receiver.borrow().clone();
        assert_eq!(view.notifications.len(), 1);
        assert_eq!(view.notifications[0].title, "Reminder");
        assert!(!view.notifications[0].read);
        execute(&client, Command::MarkNotificationRead { id: notice.id }).await;
        tokio::time::timeout(Duration::from_secs(10), async {
            while !receiver.borrow().notifications[0].read {
                receiver.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(notices(&client).await[0].read);
        execute(
            &client,
            Command::DismissNotifications {
                ids: vec![notice.id],
            },
        )
        .await;
        tokio::time::timeout(Duration::from_secs(10), async {
            while !receiver.borrow().notifications.is_empty() {
                receiver.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(notices(&client).await.is_empty());
        stop.cancel();
        watching.await.unwrap().unwrap();
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        assert!(notices(&Client::new(node.local())).await.is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn publishing_respects_plugin_permissions_and_scope() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadStorage],
        )
        .await;
        let request = client
            .prepare(Command::PublishNotification {
                package: context.package.clone(),
                content: draft(),
            })
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            1,
            &[Action::Notify],
        )
        .await;
        let mut content = draft();
        content.session = Some(SessionId::new());
        let request = client
            .prepare(Command::PublishNotification {
                package: context.package.clone(),
                content,
            })
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        let request = client
            .prepare(Command::DismissNotifications {
                ids: vec![NotificationId::new()],
            })
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        let mut package = context.package.clone();
        package.name = "another".into();
        let request = client
            .prepare(Command::PublishNotification {
                package,
                content: draft(),
            })
            .with_plugin(context);
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        assert!(notices(&client).await.is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn scheduled_callback_publishes_without_a_panel() {
    use sailry_protocol::dispatch as jobs;
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::Notify, Action::Dispatch],
        )
        .await;
        dispatch::run(
            &client,
            &context,
            jobs::Command::SaveHandler(jobs::Handler {
                name: "notify".into(),
                revision: 0,
                enabled: true,
                source: jobs::Source {
                    package: context.package.name.clone(),
                    topic: "reminder".into(),
                },
                queue: "reminders".into(),
                callback: jobs::Callback {
                    completion: jobs::Completion::Command,
                    scope: jobs::Scope::default(),
                    command: Box::new(Command::PublishNotification {
                        package: context.package.clone(),
                        content: draft(),
                    }),
                    bindings: std::collections::BTreeMap::from([(
                        "/data/content/title".into(),
                        "/payload/title".into(),
                    )]),
                },
            }),
        )
        .await;
        dispatch::run(
            &client,
            &context,
            jobs::Command::SaveSchedule(jobs::Schedule {
                id: ScheduleId::new(),
                revision: 0,
                enabled: true,
                topic: "reminder".into(),
                payload: serde_json::json!({"title":"Scheduled reminder"}),
                timing: jobs::Timing::Once {
                    at_ms: chrono::Utc::now().timestamp_millis() + 50,
                },
                next_ms: None,
            }),
        )
        .await;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let current = notices(&client).await;
                if !current.is_empty() {
                    assert_eq!(current.len(), 1);
                    assert_eq!(current[0].content.title, "Scheduled reminder");
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
