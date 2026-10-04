use super::*;
use sailry_protocol::{
    dispatch as jobs, notification,
    plugin::{Action, Context, transaction::Operation},
};
use serde_json::json;

fn write(key: &str, value: i32, revision: u64) -> Operation {
    Operation::Write {
        key: key.into(),
        value: json!(value),
        expected_revision: revision,
    }
}

fn notice(title: &str) -> Operation {
    Operation::Notify(notification::Draft {
        title: title.into(),
        message: "Updated".into(),
        kind: notification::Kind::Info,
        session: None,
    })
}

fn request(client: &Client, context: &Context, operations: Vec<Operation>) -> Request {
    client
        .prepare(Command::PluginTransaction { operations })
        .with_plugin(context.clone())
}

async fn value(client: &Client, context: &Context) -> plugin::storage::Entry {
    let Output::PluginValue(entry) = client
        .execute(
            client
                .prepare(Command::ReadPluginValue { key: "item".into() })
                .with_plugin(context.clone()),
        )
        .await
        .unwrap()
    else {
        panic!("value expected")
    };
    entry
}

async fn snapshot(client: &Client) -> Snapshot {
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    snapshot
}

#[tokio::test]
async fn rolls_back_values_schedules_and_events() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[
                Action::ReadStorage,
                Action::WriteStorage,
                Action::Dispatch,
                Action::Notify,
            ],
        )
        .await;
        let before = snapshot(&client).await;
        let mut updates = client.subscribe().await.unwrap();
        let mut projection = Projection::new(client.target(), 1);
        assert_eq!(
            projection.apply(1, updates.next().await.unwrap()).unwrap(),
            Apply::Applied
        );
        let schedule = jobs::Schedule {
            id: ScheduleId::new(),
            revision: 0,
            enabled: false,
            topic: "due".into(),
            payload: json!(null),
            timing: jobs::Timing::Once { at_ms: 0 },
            next_ms: None,
        };
        let failed = request(
            &client,
            &context,
            vec![
                write("item", 1, 0),
                Operation::Dispatch(jobs::Command::SaveSchedule(schedule.clone())),
                notice(""),
            ],
        );
        assert_eq!(
            client.execute(failed.clone()).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            client.execute(failed).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert!(!value(&client, &context).await.present);
        assert_eq!(snapshot(&client).await, before);
        let jobs::Output::Schedules(schedules) =
            dispatch::run(&client, &context, jobs::Command::ListSchedules).await
        else {
            panic!("schedules expected")
        };
        assert!(schedules.is_empty());
        let original = request(
            &client,
            &context,
            vec![
                write("item", 1, 0),
                Operation::Dispatch(jobs::Command::SaveSchedule(schedule)),
                notice("Saved"),
            ],
        );
        let result = client.execute(original.clone()).await.unwrap();
        let Output::PluginTransaction(outputs) = &result else {
            panic!("transaction expected")
        };
        assert_eq!(outputs.len(), 3);
        assert_eq!(client.execute(original.clone()).await.unwrap(), result);
        assert_eq!(value(&client, &context).await.revision, 1);
        let committed = snapshot(&client).await;
        assert_eq!(committed.notifications.len(), 1);
        assert_eq!(committed.cursor, before.cursor + 3);
        for _ in 0..3 {
            let update = tokio::time::timeout(Duration::from_secs(5), updates.next())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(projection.apply(1, update).unwrap(), Apply::Applied);
        }
        assert_eq!(projection.snapshot().unwrap(), &committed);
        // A late revision failure must roll back an earlier notice as well.
        let stale = request(
            &client,
            &context,
            vec![notice("Not committed"), write("item", 2, 0)],
        );
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(snapshot(&client).await, committed);
        drop(updates);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(original).await.unwrap(), result);
        assert_eq!(value(&client, &context).await.revision, 1);
        assert_eq!(
            snapshot(&client).await.notifications,
            committed.notifications
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn checks_every_grant_and_scope_before_mutation() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        let denied = request(
            &client,
            &context,
            vec![write("item", 1, 0), notice("Denied")],
        );
        assert_eq!(
            client.execute(denied).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        assert!(!value(&client, &context).await.present);
        let denied = client.prepare(Command::PluginTransaction {
            operations: vec![write("item", 1, 0)],
        });
        assert_eq!(
            client.execute(denied).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        for operations in [
            vec![],
            vec![write("item", 1, 0); plugin::transaction::MAX_OPERATIONS + 1],
            vec![Operation::Dispatch(jobs::Command::ListHandlers)],
        ] {
            assert_eq!(
                client
                    .execute(request(&client, &context, operations))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        let operations = vec![Operation::Write {
            key: "item".into(),
            value: json!("x".repeat(plugin::transaction::MAX_BYTES)),
            expected_revision: 0,
        }];
        assert_eq!(
            client
                .execute(request(&client, &context, operations))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let context = actions::install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            1,
            &[Action::ReadStorage, Action::WriteStorage, Action::Notify],
        )
        .await;
        let Operation::Notify(mut draft) = notice("Wrong session") else {
            unreachable!()
        };
        draft.session = Some(SessionId::new());
        assert_eq!(
            client
                .execute(request(
                    &client,
                    &context,
                    vec![write("item", 1, 0), Operation::Notify(draft)]
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert!(!value(&client, &context).await.present);
        assert!(snapshot(&client).await.notifications.is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
