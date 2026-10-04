use super::*;
use sailry_protocol::{dispatch as jobs, plugin::Context};
use serde_json::{Value, json};

async fn install(client: &Client) -> Context {
    let package = info(
        execute(
            client,
            Command::ReadPlugin {
                name: "reminders".into(),
            },
        )
        .await,
    );
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: package.summary.reference(),
        worktree: None,
        session: None,
    }
}
fn request(client: &Client, context: &Context, handler: &str, input: Value) -> Request {
    client
        .prepare(Command::CallPlugin {
            handler: handler.into(),
            input,
        })
        .with_plugin(context.clone())
}
async fn call(client: &Client, context: &Context, handler: &str, input: Value) -> Value {
    let Output::PluginResult(value) = client
        .execute(request(client, context, handler, input))
        .await
        .unwrap()
    else {
        panic!("callback result expected")
    };
    value
}
async fn items(client: &Client, context: &Context) -> Vec<Value> {
    let value = call(client, context, "list", json!({})).await;
    value["Ok"]["items"]
        .as_array()
        .expect("reminder page")
        .clone()
}
fn draft(id: ScheduleId, at: Option<i64>) -> Value {
    json!({"id":id,"revision":"0","project":null,"title":"Review","message":"Review the document","due_ms":at,"completed":false})
}
async fn storage(client: &Client, context: &Context, command: Command) -> plugin::storage::Entry {
    let Output::PluginValue(entry) = client
        .execute(client.prepare(command).with_plugin(context.clone()))
        .await
        .unwrap()
    else {
        panic!("reminder value expected")
    };
    entry
}
async fn notices(client: &Client) -> Vec<notification::Notice> {
    let Output::Snapshot(snapshot) = execute(client, Command::Snapshot).await else {
        panic!("snapshot expected")
    };
    snapshot.notifications
}

#[tokio::test]
async fn edits_and_removes_through_public_callbacks() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _worktree) = fixture(remote).await;
        let context = install(&client).await;
        let id = ScheduleId::new();
        let original = request(&client, &context, "save", draft(id, None));
        let result = client.execute(original.clone()).await.unwrap();
        let Output::PluginResult(saved) = &result else {
            panic!("callback result expected")
        };
        assert_eq!(saved["Ok"]["revision"], "1", "{saved}");
        assert_eq!(client.execute(original).await.unwrap(), result);
        let mut item = items(&client, &context).await.remove(0);
        assert_eq!(item["id"], json!(id));
        item["completed"] = json!(true);
        assert_eq!(
            call(&client, &context, "save", item.clone()).await["Ok"]["revision"],
            "2"
        );
        assert_eq!(
            call(&client, &context, "save", item).await["Err"]["code"],
            "revision_conflict"
        );
        assert_eq!(items(&client, &context).await[0]["completed"], true);
        assert_eq!(
            call(&client, &context, "remove", json!({"id":id,"revision":"2"})).await["Ok"]["removed"],
            true
        );
        assert!(items(&client, &context).await.is_empty());
        let jobs::Output::Schedules(schedules) =
            dispatch::run(&client, &context, jobs::Command::ListSchedules).await
        else {
            panic!("schedules expected")
        };
        assert!(schedules.is_empty());
        let mut invalid = draft(ScheduleId::new(), None);
        invalid["title"] = json!(" ");
        assert_eq!(
            call(&client, &context, "save", invalid).await["Err"]["code"],
            "invalid_request"
        );
        assert!(items(&client, &context).await.is_empty());
        assert!(notices(&client).await.is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn selects_and_preserves_project_associations() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = install(&client).await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let project = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.id == worktree)
            .unwrap()
            .project
            .unwrap();
        let root = directory.path().join("other");
        fs::create_dir(&root).unwrap();
        let Output::Project(other) = execute(
            &client,
            Command::RegisterProject {
                name: "Other".into(),
                path: root.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let unassigned = ScheduleId::new();
        let saved = call(&client, &context, "save", draft(unassigned, None)).await;
        assert_eq!(saved["Ok"]["project"], Value::Null, "{saved}");
        assert_eq!(saved["Ok"]["revision"], "1");
        let id = ScheduleId::new();
        let mut assigned = draft(id, None);
        assigned["project"] = json!(project);
        let saved = call(&client, &context, "save", assigned).await;
        assert_eq!(saved["Ok"]["project"], json!(project), "{saved}");
        let mut item = items(&client, &context)
            .await
            .into_iter()
            .find(|item| item["id"] == json!(id))
            .unwrap();
        item["title"] = json!("Edited");
        let saved = call(&client, &context, "save", item).await;
        assert_eq!(saved["Ok"]["project"], json!(project), "{saved}");
        assert_eq!(saved["Ok"]["revision"], "2");
        let mut item = saved["Ok"].clone();
        item["project"] = json!(other.id);
        let saved = call(&client, &context, "save", item).await;
        assert_eq!(saved["Ok"]["project"], json!(other.id), "{saved}");
        assert_eq!(saved["Ok"]["revision"], "3");
        let mut item = saved["Ok"].clone();
        item["project"] = Value::Null;
        let saved = call(&client, &context, "save", item).await;
        assert_eq!(saved["Ok"]["project"], Value::Null, "{saved}");
        assert_eq!(saved["Ok"]["revision"], "4");
        let before = items(&client, &context).await;
        assert_eq!(before.len(), 2);
        assert!(before.iter().all(|item| item["project"] == Value::Null));
        let mut unknown = saved["Ok"].clone();
        unknown["project"] = json!(ProjectId::new());
        assert_eq!(
            call(&client, &context, "save", unknown).await["Err"]["code"],
            "not_found"
        );
        assert_eq!(items(&client, &context).await, before);
        let missing = ScheduleId::new();
        let mut invalid = draft(missing, None);
        invalid.as_object_mut().unwrap().remove("project");
        assert_eq!(
            call(&client, &context, "save", invalid.clone()).await["Err"]["code"],
            "invalid_request"
        );
        let key = format!("reminder/{missing}");
        assert!(
            !storage(
                &client,
                &context,
                Command::ReadPluginValue { key: key.clone() }
            )
            .await
            .present
        );
        // Seed a schema-valid record with a mismatched key identity; callbacks must
        // reject it without rewriting the persisted value or revision.
        invalid.as_object_mut().unwrap().remove("revision");
        invalid["id"] = json!(ScheduleId::new());
        invalid["notified_ms"] = Value::Null;
        let original = storage(
            &client,
            &context,
            Command::WritePluginValue {
                key: key.clone(),
                value: invalid,
                expected_revision: 0,
            },
        )
        .await;
        assert_eq!(original.revision, 1);
        assert_eq!(original.value["project"], Value::Null);
        assert_ne!(original.value["id"], json!(missing));
        assert_eq!(
            call(&client, &context, "list", json!({})).await["Err"]["message"],
            "Reminder storage is incompatible"
        );
        let mut replacement = draft(missing, None);
        replacement["revision"] = json!("1");
        for (handler, input) in [
            ("save", replacement),
            ("remove", json!({"id":missing,"revision":"1"})),
        ] {
            let rejected = call(&client, &context, handler, input).await;
            assert_eq!(rejected["Err"]["code"], "invalid_request");
            assert_eq!(
                rejected["Err"]["message"],
                "Reminder storage is incompatible"
            );
        }
        assert_eq!(
            storage(&client, &context, Command::ReadPluginValue { key }).await,
            original
        );
        let jobs::Output::Schedules(schedules) =
            dispatch::run(&client, &context, jobs::Command::ListSchedules).await
        else {
            panic!("schedules expected")
        };
        assert!(schedules.iter().all(|schedule| schedule.id != missing));
        assert!(notices(&client).await.is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn delivers_once_after_restart_without_a_view() {
    for remote in [false, true] {
        let (directory, node, controller, client, _worktree) = fixture(remote).await;
        let context = install(&client).await;
        let id = ScheduleId::new();
        let due = chrono::Utc::now().timestamp_millis() + 150;
        let saved = call(&client, &context, "save", draft(id, Some(due))).await;
        assert_eq!(saved["Ok"]["revision"], "1", "{saved}");
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        tokio::time::timeout(Duration::from_secs(10), async {
            while notices(&client).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let delivered = notices(&client).await;
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].content.title, "Review");
        let item = items(&client, &context).await.remove(0);
        assert_eq!(item["revision"], "2");
        assert!(item["notified_ms"].as_i64().is_some());
        assert_eq!(
            call(&client, &context, "notify", json!({"id":id,"revision":"1"})).await["skipped"],
            true
        );
        assert_eq!(notices(&client).await, delivered);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn ignores_stale_delivery_callbacks() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _worktree) = fixture(remote).await;
        let context = install(&client).await;
        let id = ScheduleId::new();
        let saved = call(
            &client,
            &context,
            "save",
            draft(id, Some(chrono::Utc::now().timestamp_millis() + 250)),
        )
        .await;
        assert!(saved.get("Ok").is_some(), "{saved}");
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "reminders".into(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(notices(&client).await.is_empty());
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "reminders".into(),
                expected_revision: 2,
                enabled: true,
            },
        )
        .await;
        tokio::time::timeout(Duration::from_secs(10), async {
            while notices(&client).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(notices(&client).await.len(), 1);
        let item = items(&client, &context).await.remove(0);
        let revision = item["revision"].as_str().unwrap().to_owned();
        assert_eq!(
            call(
                &client,
                &context,
                "remove",
                json!({"id":id,"revision":revision})
            )
            .await["Ok"]["removed"],
            true
        );
        assert_eq!(
            call(
                &client,
                &context,
                "notify",
                json!({"id":id,"revision":revision})
            )
            .await["skipped"],
            true
        );
        assert_eq!(notices(&client).await.len(), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
