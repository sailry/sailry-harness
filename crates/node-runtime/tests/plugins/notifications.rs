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
        presentation: Some(sailry_protocol::notification::Presentation {
            heading: "Reminder".into(),
            locales: std::collections::BTreeMap::from([("zh-CN".into(), "提醒".into())]),
            persistent: true,
        }),
        details: Some(sailry_protocol::notification::Details::Text {
            body: "Workflow result".into(),
        }),
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
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
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
        let node = Node::start_empty_with_plugin_catalog(
            directory.path().join("node"),
            "http://127.0.0.1:9/",
        )
        .await
        .unwrap();
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
        assert_eq!(
            view.notifications[0].presentation,
            notice.content.presentation
        );
        assert!(!view.notifications[0].read);
        execute(
            &client,
            Command::MarkNotificationsRead {
                ids: vec![notice.id],
            },
        )
        .await;
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
            Command::DismissReadNotifications {
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
        let node = Node::start_empty_with_plugin_catalog(
            directory.path().join("node"),
            "http://127.0.0.1:9/",
        )
        .await
        .unwrap();
        assert!(notices(&Client::new(node.local())).await.is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn publishing_respects_plugin_permissions_and_scope() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
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
        for command in [
            Command::MarkNotificationsRead {
                ids: vec![NotificationId::new()],
            },
            Command::DismissReadNotifications {
                ids: vec![NotificationId::new()],
            },
        ] {
            let request = client.prepare(command).with_plugin(context.clone());
            assert_eq!(
                client.execute(request).await.unwrap_err().code,
                ErrorCode::PermissionDenied
            );
        }
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
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
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

#[tokio::test]
async fn system_reports_survive_reconnect_without_duplicates() {
    use sailry_protocol::notification::{
        Details, DiagnosticReason, SystemDiagnostic, SystemSource,
    };
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::Notify],
        )
        .await;
        let mut report = SystemDiagnostic::new(
            RequestId::new(),
            SystemSource::GitInspection,
            ErrorCode::Unavailable,
        );
        report.worktree = Some(worktree);
        report.operation = Some("InspectGit".into());
        let mut updates = client.subscribe().await.unwrap();
        let _ = updates.next().await.unwrap();
        client.report_diagnostic(report.clone()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Update::Event(event) = updates.next().await.unwrap()
                    && let Event::NotificationChanged(notice) = event.event
                    && matches!(notice.content.details, Some(Details::System(_)))
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        client.report_diagnostic(report.clone()).await.unwrap();
        // A fresh transport command cannot duplicate the same logical failure either.
        client
            .execute(client.prepare(Command::ReportDiagnostic(report.clone())))
            .await
            .unwrap();
        let systems = |items: Vec<Notice>| {
            items
                .into_iter()
                .filter(|notice| matches!(notice.content.details, Some(Details::System(_))))
                .collect::<Vec<_>>()
        };
        assert_eq!(systems(notices(&client).await).len(), 1);
        let forbidden = client
            .prepare(Command::ReportDiagnostic(report.clone()))
            .with_plugin(context);
        assert_eq!(
            client.execute(forbidden).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        let mut invalid = report.clone();
        invalid.incident = RequestId::new();
        invalid.worktree = Some(WorktreeId::new());
        assert_eq!(
            client.report_diagnostic(invalid).await.unwrap_err().code,
            ErrorCode::NotFound
        );
        let mut cancelled = report.clone();
        cancelled.incident = RequestId::new();
        cancelled.reason = DiagnosticReason::Cancelled;
        client.report_diagnostic(cancelled).await.unwrap();
        assert_eq!(systems(notices(&client).await).len(), 1);
        drop(updates);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start_empty_with_plugin_catalog(
            directory.path().join("node"),
            "http://127.0.0.1:9/",
        )
        .await
        .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        client.report_diagnostic(report.clone()).await.unwrap();
        let persisted = systems(notices(&client).await);
        assert_eq!(persisted.len(), 1);
        assert_eq!(
            persisted[0].content.details,
            Some(Details::System(report.clone()))
        );
        execute(
            &client,
            Command::MarkNotificationRead {
                id: persisted[0].id,
            },
        )
        .await;
        execute(
            &client,
            Command::DismissReadNotifications {
                ids: vec![persisted[0].id],
            },
        )
        .await;
        client.report_diagnostic(report).await.unwrap();
        assert!(systems(notices(&client).await).is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
