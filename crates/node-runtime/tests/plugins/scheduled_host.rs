use super::*;
use sailry_protocol::{dispatch as jobs, plugin::Context};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Notify;

#[path = "scheduled_host/admission.rs"]
mod admission;
#[path = "scheduled_host/stop.rs"]
mod stop;

fn copy_package(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_package(&entry.path(), &target.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
        }
    }
}

async fn installed(client: &Client, directory: &Path, worktree: WorktreeId) -> Context {
    copy_package(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/scheduled-tasks"),
        &directory.join("source/tasks"),
    );
    let package = info(
        execute(
            client,
            Command::InstallPlugin {
                worktree,
                path: "tasks".into(),
                name: "scheduled-tasks".into(),
                expected_revision: 1,
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

async fn update(
    client: &Client,
    directory: &Path,
    tree: WorktreeId,
    expected_revision: u64,
) -> plugin::Reference {
    let manifest = directory.join("source/tasks/plugin.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["version"] = json!("0.2.0");
    fs::write(&manifest, value.to_string()).unwrap();
    info(
        execute(
            client,
            Command::InstallPlugin {
                worktree: tree,
                path: "tasks".into(),
                name: "scheduled-tasks".into(),
                expected_revision,
            },
        )
        .await,
    )
    .summary
    .reference()
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
async fn ok(client: &Client, context: &Context, handler: &str, input: Value) -> Value {
    let value = call(client, context, handler, input).await;
    value.get("Ok").unwrap_or_else(|| panic!("{value}")).clone()
}
async fn config(client: &Client, endpoint: String) -> SessionConfig {
    let model = completion::configure(client, endpoint).await;
    SessionConfig {
        assistant: None,
        resource: None,
        provider: model.split('/').next().unwrap().parse().unwrap(),
        model: "fixture".into(),
        effort: Effort::Default,
        mode: WorkMode::Code,
        permission: Permission::Ask,
        credential: None,
    }
}
fn draft(config: Option<SessionConfig>) -> Value {
    json!({"id":ScheduleId::new(),"revision":"0","name":"Scheduled fixture","prompt":"Run the scheduled review",
        "project":null,"worktree":null,"config":config,"enabled":false,"queue":"reviews",
        "timing":{"kind":"once","data":{"at_ms":chrono::Utc::now().timestamp_millis()+600_000}}})
}
async fn history(client: &Client, context: &Context) -> Vec<Value> {
    ok(client, context, "history", json!({})).await["items"]
        .as_array()
        .unwrap()
        .clone()
}
async fn wait(client: &Client, context: &Context, id: &Value, status: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(item) = history(client, context)
                .await
                .into_iter()
                .find(|item| &item["job"]["id"] == id)
            {
                if item["job"]["status"] == status {
                    return item;
                }
                assert!(
                    matches!(item["job"]["status"].as_str(), Some("queued" | "running")),
                    "{item}"
                );
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn callbacks_manage_pending_tasks() {
    for remote in [false, true] {
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, "http://127.0.0.1:1/v1".into()).await;
        let mut input = draft(Some(configuration));
        input["enabled"] = json!(true);
        let save = request(&client, &context, "save", input.clone());
        let output = client.execute(save.clone()).await.unwrap();
        assert_eq!(client.execute(save).await.unwrap(), output);
        let mut saved = ok(&client, &context, "list", json!({})).await["items"][0].clone();
        assert_eq!(saved["revision"], "1");
        assert_eq!(
            call(&client, &context, "save", input).await["Err"]["code"],
            "revision_conflict"
        );
        saved["name"] = json!("Renamed task");
        let updated = ok(&client, &context, "save", saved).await;
        assert_eq!(updated["revision"], "2");
        let jobs::Output::Schedules(schedules) =
            dispatch::run(&client, &context, jobs::Command::ListSchedules).await
        else {
            panic!("schedules expected")
        };
        assert_eq!(schedules.len(), 1);
        assert!(schedules[0].next_ms.is_some());
        assert_eq!(
            ok(&client, &context, "list", json!({})).await["items"][0]["name"],
            "Renamed task"
        );
        let removal = request(
            &client,
            &context,
            "remove",
            json!({"id":updated["id"],"revision":"2"}),
        );
        let removed = client.execute(removal.clone()).await.unwrap();
        assert_eq!(client.execute(removal).await.unwrap(), removed);
        assert!(history(&client, &context).await.is_empty());
        assert!(
            ok(&client, &context, "list", json!({})).await["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            matches!(dispatch::run(&client, &context, jobs::Command::ListHandlers).await, jobs::Output::Handlers(items) if items.is_empty())
        );
        assert!(
            matches!(dispatch::run(&client, &context, jobs::Command::ListSchedules).await, jobs::Output::Schedules(items) if items.is_empty())
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_missing_models_without_mutating_records() {
    for remote in [false, true] {
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, "http://127.0.0.1:1/v1".into()).await;
        let Output::Snapshot(before) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert!(before.defaults.config.is_none());
        let unconfigured = draft(None);
        assert_eq!(
            call(&client, &context, "save", unconfigured.clone()).await["Err"]["code"],
            "not_configured"
        );
        assert!(
            ok(&client, &context, "list", json!({})).await["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            matches!(dispatch::run(&client,&context,jobs::Command::ListHandlers).await,jobs::Output::Handlers(items) if items.is_empty())
        );
        assert!(
            matches!(dispatch::run(&client,&context,jobs::Command::ListSchedules).await,jobs::Output::Schedules(items) if items.is_empty())
        );

        let key = format!("task/{}", unconfigured["id"].as_str().unwrap());
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::WritePluginValue {
                            key: key.clone(),
                            value: unconfigured.clone(),
                            expected_revision: 0,
                        })
                        .with_plugin(context.clone()),
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        let Output::PluginValue(absent) = client
            .execute(
                client
                    .prepare(Command::ReadPluginValue { key: key.clone() })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("value expected")
        };
        assert!(!absent.present);
        assert_eq!(absent.revision, 0);

        // A stored task must satisfy its schema even when its model is unavailable.
        let mut missing_model = unconfigured.clone();
        missing_model["config"] = serde_json::to_value(&configuration).unwrap();
        missing_model["config"]["model"] = json!("unavailable");
        let Output::PluginValue(written) = client
            .execute(
                client
                    .prepare(Command::WritePluginValue {
                        key: key.clone(),
                        value: missing_model,
                        expected_revision: 0,
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("value expected")
        };
        assert_eq!(
            call(&client, &context, "run", json!({"id":unconfigured["id"]})).await["Err"]["code"],
            "not_configured"
        );
        let Output::PluginValue(retained) = client
            .execute(
                client
                    .prepare(Command::ReadPluginValue { key })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("value expected")
        };
        assert_eq!(retained, written);
        assert!(history(&client, &context).await.is_empty());

        let explicit = ok(
            &client,
            &context,
            "save",
            draft(Some(configuration.clone())),
        )
        .await;
        assert_eq!(
            serde_json::from_value::<SessionConfig>(explicit["config"].clone()).unwrap(),
            configuration
        );
        let jobs::Output::Handlers(handlers) =
            dispatch::run(&client, &context, jobs::Command::ListHandlers).await
        else {
            panic!("handlers expected")
        };
        assert_eq!(handlers.len(), 1);
        assert!(
            matches!(&*handlers[0].callback.command,Command::StartSession(input) if input.config.as_ref()==Some(&configuration))
        );
        let mut invalid = draft(Some(configuration));
        invalid["config"]["model"] = json!("unavailable");
        assert_eq!(
            call(&client, &context, "save", invalid).await["Err"]["code"],
            "not_configured"
        );
        assert_eq!(
            ok(&client, &context, "list", json!({})).await["items"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let Output::Snapshot(after) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(after.defaults, before.defaults);
        assert_eq!(after.sessions, before.sessions);
        assert_eq!(after.turns, before.turns);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn same_group_runs_preserve_captured_history() {
    for remote in [false, true] {
        let start = Arc::new(Notify::new());
        let finish = Arc::new(Notify::new());
        let server = model_fixture::Server::held(start.clone(), finish.clone()).await;
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let mut context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, server.endpoint.clone()).await;
        let mut task = ok(&client, &context, "save", draft(Some(configuration))).await;
        let enqueue = request(&client, &context, "run", json!({"id":task["id"]}));
        let output = client.execute(enqueue.clone()).await.unwrap();
        assert_eq!(client.execute(enqueue).await.unwrap(), output);
        let Output::PluginResult(result) = output else {
            panic!("callback result expected")
        };
        let first = result["Ok"].clone();
        let second = ok(&client, &context, "run", json!({"id":task["id"]})).await;
        // Both held model calls must start before either one is released.
        server.wait_count(2).await;
        let running = wait(&client, &context, &first["id"], "running").await;
        let second_running = wait(&client, &context, &second["id"], "running").await;
        assert!(running["session"].is_string());
        assert_ne!(running["session"], second_running["session"]);
        assert_eq!(running["job"]["queue"], second_running["job"]["queue"]);
        task["prompt"] = json!("Edited after admission");
        task = ok(&client, &context, "save", task).await;
        // Admitted callbacks retain their original grants and arguments;
        // subsequent deliveries use the currently installed package.
        let previous = context.package.clone();
        context.package = update(&client, directory.path(), tree, 2).await;
        assert_ne!(context.package.digest, previous.digest);
        ok(
            &client,
            &context,
            "remove",
            json!({"id":task["id"],"revision":task["revision"]}),
        )
        .await;
        assert_eq!(history(&client, &context).await.len(), 2);
        start.notify_waiters();
        finish.notify_one();
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if history(&client, &context)
                    .await
                    .iter()
                    .any(|item| item["job"]["status"] == "completed")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        finish.notify_one();
        wait(&client, &context, &first["id"], "completed").await;
        wait(&client, &context, &second["id"], "completed").await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.sessions.len(), 2);
        assert!(
            snapshot
                .sessions
                .iter()
                .all(|session| session.project.is_none())
        );
        assert_ne!(snapshot.sessions[0].worktree, snapshot.sessions[1].worktree);
        for item in [&running, &second_running] {
            let session = serde_json::from_value(item["session"].clone()).unwrap();
            let Output::Conversation(history) = execute(
                &client,
                Command::ReadConversation {
                    session,
                    before: None,
                    limit: 10,
                },
            )
            .await
            else {
                panic!("history expected")
            };
            assert_eq!(history.page.runs[0].status, conversation::Status::Completed);
            let encoded = serde_json::to_string(&history).unwrap();
            assert!(encoded.contains("Run the scheduled review"));
            assert!(!encoded.contains("Edited after admission"));
        }
        // The project catalog must not reveal private session workspaces.
        let catalog = ok(&client, &context, "projects", json!({})).await;
        assert_eq!(catalog["projects"].as_array().unwrap().len(), 1);
        assert_eq!(catalog["worktrees"].as_array().unwrap().len(), 1);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn resumes_disabled_delivery() {
    for remote in [false, true] {
        let server = model_fixture::Server::markdown("Scheduled result".into()).await;
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let mut context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, server.endpoint.clone()).await;
        let mut task = draft(Some(configuration));
        task["enabled"] = json!(true);
        task["timing"]["data"]["at_ms"] = json!(chrono::Utc::now().timestamp_millis() + 800);
        ok(&client, &context, "save", task).await;
        execute(
            &client,
            Command::SetPluginEnabled {
                name: context.package.name.clone(),
                expected_revision: 2,
                enabled: false,
            },
        )
        .await;
        tokio::time::sleep(Duration::from_millis(850)).await;
        assert!(server.requests.lock().unwrap().is_empty());
        let manifest = directory.path().join("source/tasks/plugin.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["version"] = json!("0.2.0");
        fs::write(&manifest, value.to_string()).unwrap();
        let updated = info(
            execute(
                &client,
                Command::InstallPlugin {
                    worktree: tree,
                    path: "tasks".into(),
                    name: context.package.name.clone(),
                    expected_revision: 3,
                },
            )
            .await,
        );
        context.package = updated.summary.reference();
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        execute(
            &client,
            Command::SetPluginEnabled {
                name: context.package.name.clone(),
                expected_revision: 4,
                enabled: true,
            },
        )
        .await;
        server.wait_count(1).await;
        let items = history(&client, &context).await;
        assert_eq!(items.len(), 1);
        wait(&client, &context, &items[0]["job"]["id"], "completed").await;
        assert!(ok(&client, &context, "list", json!({})).await["items"][0]["next_ms"].is_null());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn admission_preserves_scope() {
    for remote in [false, true] {
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, "http://127.0.0.1:1/v1".into()).await;
        let task = ok(&client, &context, "save", draft(Some(configuration))).await;
        let jobs::Output::Handlers(mut handlers) =
            dispatch::run(&client, &context, jobs::Command::ListHandlers).await
        else {
            panic!("handlers expected")
        };
        let mut handler = handlers.remove(0);
        let Command::StartSession(start) = &mut *handler.callback.command else {
            panic!("start callback expected")
        };
        // Exercise a generic captured callback independently of current task validation.
        // Missing config fails before session creation when Node defaults are absent.
        start.config = None;
        dispatch::run(&client, &context, jobs::Command::SaveHandler(handler)).await;
        let jobs::Output::Job(enqueued) = dispatch::run(
            &client,
            &context,
            jobs::Command::Enqueue {
                handler: task["id"].as_str().unwrap().into(),
                payload: json!({"name":task["name"]}),
            },
        )
        .await
        else {
            panic!("job expected")
        };
        let job = serde_json::to_value(enqueued).unwrap();
        let failed = wait(&client, &context, &job["id"], "failed").await;
        assert_eq!(failed["job"]["error"]["code"], "not_configured");
        assert!(failed["session"].is_null());
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert!(snapshot.sessions.is_empty());
        let mut scoped = context.clone();
        scoped.worktree = Some(tree);
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ReadProjectCatalog)
                        .with_plugin(scoped)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let foreign = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "progress".into(),
                },
            )
            .await,
        );
        assert!(
            !foreign
                .extension
                .as_ref()
                .unwrap()
                .actions
                .contains(&plugin::Action::ReadProjects)
        );
        let mut other = context.clone();
        other.package = foreign.summary.reference();
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ReadProjectCatalog)
                        .with_plugin(other.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        // Reminder management declares dispatch, but cannot read another package's job.
        other.package = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "reminders".into(),
                },
            )
            .await,
        )
        .summary
        .reference();
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::Dispatch {
                            package: other.package.clone(),
                            action: jobs::Command::ReadResult {
                                id: serde_json::from_value(job["id"].clone()).unwrap()
                            }
                        })
                        .with_plugin(other)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn long_prompts_are_paginated_within_the_callback_budget() {
    for remote in [false, true] {
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, "http://127.0.0.1:1/v1".into()).await;
        let mut expected = std::collections::BTreeSet::new();
        for _ in 0..8 {
            let mut input = draft(Some(configuration.clone()));
            input["prompt"] = json!("x".repeat(32768));
            let saved = ok(&client, &context, "save", input).await;
            expected.insert(saved["id"].as_str().unwrap().to_owned());
        }
        let mut found = std::collections::BTreeSet::new();
        let mut after = Value::Null;
        let mut pages = 0;
        loop {
            let page = ok(&client, &context, "list", json!({"after":after})).await;
            assert!(serde_json::to_vec(&page).unwrap().len() < plugin::host::MAX_DATA_BYTES);
            for item in page["items"].as_array().unwrap() {
                assert_eq!(item["prompt"].as_str().unwrap().len(), 32768);
                assert!(found.insert(item["id"].as_str().unwrap().to_owned()));
            }
            pages += 1;
            after = page["after"].clone();
            if after.is_null() {
                break;
            }
        }
        assert!(pages > 1);
        assert_eq!(found, expected);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn interrupted_turns_are_not_replayed_on_restart() {
    for remote in [false, true] {
        let start = Arc::new(Notify::new());
        let finish = Arc::new(Notify::new());
        let server = model_fixture::Server::held(start.clone(), finish.clone()).await;
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, server.endpoint.clone()).await;
        let task = ok(&client, &context, "save", draft(Some(configuration))).await;
        let job = ok(&client, &context, "run", json!({"id":task["id"]})).await;
        server.wait_count(1).await;
        let running = wait(&client, &context, &job["id"], "running").await;
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let items = history(&client, &context).await;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["session"], running["session"]);
        assert!(
            matches!(
                items[0]["job"]["status"].as_str(),
                Some("unknown" | "cancelled")
            ),
            "{items:?}"
        );
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        start.notify_one();
        finish.notify_one();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
