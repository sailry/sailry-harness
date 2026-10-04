use super::*;
use sailry_protocol::plugin::{
    Action, Context,
    storage::{Entry, Page},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[path = "storage/indexed.rs"]
mod indexed;
#[path = "storage/logical.rs"]
mod logical;

fn request(client: &Client, context: &Context, command: Command) -> Request {
    client.prepare(command).with_plugin(context.clone())
}

fn entry(output: Output) -> Entry {
    let Output::PluginValue(entry) = output else {
        panic!("plugin value expected")
    };
    entry
}

fn page(output: Output) -> Page {
    let Output::PluginKeys(page) = output else {
        panic!("plugin key page expected")
    };
    page
}

async fn run(client: &Client, context: &Context, command: Command) -> Output {
    client
        .execute(request(client, context, command))
        .await
        .unwrap()
}

async fn read(client: &Client, context: &Context, key: &str) -> Entry {
    entry(
        run(
            client,
            context,
            Command::ReadPluginValue { key: key.into() },
        )
        .await,
    )
}

async fn write(
    client: &Client,
    context: &Context,
    key: &str,
    value: Value,
    expected_revision: u64,
) -> Entry {
    entry(
        run(
            client,
            context,
            Command::WritePluginValue {
                key: key.into(),
                value,
                expected_revision,
            },
        )
        .await,
    )
}

async fn install_named(
    client: &Client,
    source: &Path,
    worktree: WorktreeId,
    name: &str,
    actions: &[Action],
) -> Context {
    let root = source.join(name);
    package(&root, "1.0.0");
    let path = root.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["name"] = json!(name);
    fs::write(&path, manifest.to_string()).unwrap();
    actions::declare(&root, actions);
    let installed = info(
        execute(
            client,
            Command::InstallPlugin {
                worktree,
                path: name.into(),
                name: name.into(),
                expected_revision: 0,
            },
        )
        .await,
    );
    Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: installed.summary.reference(),
        worktree: Some(worktree),
        session: None,
    }
}

#[tokio::test]
async fn isolates_namespaces_and_grants() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let source = directory.path().join("source");
        let first = actions::install_actions(
            &client,
            &source.join("package"),
            worktree,
            0,
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        let second =
            install_named(&client, &source, worktree, "other", &[Action::ReadStorage]).await;
        assert_eq!(
            write(&client, &first, "shared", json!("first"), 0)
                .await
                .revision,
            1
        );
        assert_eq!(read(&client, &second, "shared").await.value, Value::Null);
        assert_eq!(
            client
                .execute(request(
                    &client,
                    &second,
                    Command::WritePluginValue {
                        key: "shared".into(),
                        value: json!("denied"),
                        expected_revision: 0,
                    },
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadPluginValue {
                    key: "shared".into()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::WritePluginValue {
                    key: "shared".into(),
                    value: json!("no provenance"),
                    expected_revision: 0,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        actions::declare(
            &source.join("other"),
            &[Action::ReadStorage, Action::WriteStorage],
        );
        let updated = info(
            execute(
                &client,
                Command::InstallPlugin {
                    worktree,
                    path: "other".into(),
                    name: "other".into(),
                    expected_revision: 1,
                },
            )
            .await,
        );
        let second = Context {
            package: updated.summary.reference(),
            ..second
        };
        assert_eq!(
            write(&client, &second, "shared", json!("second"), 0)
                .await
                .revision,
            1
        );
        assert_eq!(read(&client, &first, "shared").await.value, json!("first"));
        assert_eq!(
            read(&client, &second, "shared").await.value,
            json!("second")
        );
        let mut missing = first.clone();
        missing.session = Some(SessionId::new());
        assert_eq!(
            client
                .execute(request(
                    &client,
                    &missing,
                    Command::ReadPluginValue {
                        key: "shared".into()
                    }
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn checks_revisions_receipts_and_pages() {
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
        assert_eq!(
            read(&client, &context, "city/alpha").await,
            Entry {
                key: "city/alpha".into(),
                revision: 0,
                value: Value::Null,
                present: false,
            }
        );
        let null = write(&client, &context, "null", Value::Null, 0).await;
        assert!(null.present);
        assert_eq!(null.value, Value::Null);
        assert_eq!(read(&client, &context, "null").await, null);
        let read_request = request(
            &client,
            &context,
            Command::ReadPluginValue {
                key: "city/alpha".into(),
            },
        );
        let read_pending = client.dispatch(read_request).await.unwrap();
        assert!(!read_pending.receipt.durable);
        read_pending.completion.await.unwrap().unwrap();
        let write_request = request(
            &client,
            &context,
            Command::WritePluginValue {
                key: "city/alpha".into(),
                value: json!({"place":"北京🙂"}),
                expected_revision: 0,
            },
        );
        let pending = client.dispatch(write_request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending);
        let first = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let RequestOutcome::Completed(result) =
                    client.outcome(&write_request).await.unwrap()
                {
                    break entry(result.unwrap());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(first.value, json!({"place":"北京🙂"}));
        assert_eq!(
            entry(client.execute(write_request.clone()).await.unwrap()),
            first
        );
        assert_eq!(read(&client, &context, "city/alpha").await, first);
        assert_eq!(
            client
                .execute(request(
                    &client,
                    &context,
                    Command::WritePluginValue {
                        key: "city/alpha".into(),
                        value: json!("stale"),
                        expected_revision: 0,
                    }
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            write(&client, &context, "city/alpha", json!("updated"), 1)
                .await
                .revision,
            2
        );
        let removed = entry(
            run(
                &client,
                &context,
                Command::RemovePluginValue {
                    key: "city/alpha".into(),
                    expected_revision: 2,
                },
            )
            .await,
        );
        assert_eq!(removed.revision, 3);
        assert_eq!(removed.value, Value::Null);
        assert_eq!(read(&client, &context, "city/alpha").await, removed);
        assert_eq!(
            entry(
                run(
                    &client,
                    &context,
                    Command::RemovePluginValue {
                        key: "never".into(),
                        expected_revision: 0,
                    }
                )
                .await
            )
            .revision,
            0
        );
        assert_eq!(
            write(&client, &context, "city/alpha", json!(1), 3)
                .await
                .revision,
            4
        );
        write(&client, &context, "city/zeta", json!(2), 0).await;
        write(&client, &context, "city/北京🙂", json!(3), 0).await;
        let first_page = page(
            run(
                &client,
                &context,
                Command::ListPluginKeys {
                    prefix: "city/".into(),
                    after: None,
                    limit: 2,
                },
            )
            .await,
        );
        assert_eq!(first_page.keys, ["city/alpha", "city/zeta"]);
        assert_eq!(first_page.after.as_deref(), Some("city/zeta"));
        let next_page = page(
            run(
                &client,
                &context,
                Command::ListPluginKeys {
                    prefix: "city/".into(),
                    after: first_page.after,
                    limit: 2,
                },
            )
            .await,
        );
        assert_eq!(next_page.keys, ["city/北京🙂"]);
        assert_eq!(next_page.after, None);
        for key in ["", "invalid\0key", &"x".repeat(129)] {
            assert_eq!(
                client
                    .execute(request(
                        &client,
                        &context,
                        Command::ReadPluginValue { key: key.into() }
                    ))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        for limit in [0, 101] {
            assert_eq!(
                client
                    .execute(request(
                        &client,
                        &context,
                        Command::ListPluginKeys {
                            prefix: String::new(),
                            after: None,
                            limit,
                        }
                    ))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        assert_eq!(
            client
                .execute(request(
                    &client,
                    &context,
                    Command::WritePluginValue {
                        key: "too-large".into(),
                        value: json!("x".repeat(plugin::storage::MAX_VALUE_BYTES)),
                        expected_revision: 0,
                    }
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(read(&client, &context, "too-large").await.revision, 0);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn keeps_values_through_restart_update_and_reinstall() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        let context = actions::install_actions(
            &client,
            &root,
            worktree,
            0,
            &[Action::ReadStorage, Action::WriteStorage],
        )
        .await;
        let original = request(
            &client,
            &context,
            Command::WritePluginValue {
                key: "saved".into(),
                value: json!({"note":"kept"}),
                expected_revision: 0,
            },
        );
        assert_eq!(
            entry(client.execute(original.clone()).await.unwrap()).revision,
            1
        );
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            read(&client, &context, "saved").await.value,
            json!({"note":"kept"})
        );
        assert_eq!(entry(client.execute(original).await.unwrap()).revision, 1);
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(root.join("plugin.json")).unwrap()).unwrap();
        manifest["version"] = json!("2.0.0");
        manifest["extensions"]["dev.sailry.platform"]["settings_schema"] =
            json!("dev.sailry.platform/settings.json");
        fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
        fs::write(
            root.join("dev.sailry.platform/settings.json"),
            json!({
                "$schema":plugin::settings::SCHEMA, "type":"object", "additionalProperties":false,
                "properties":{"note":{"type":"string", "default":""}}
            })
            .to_string(),
        )
        .unwrap();
        let updated = info(execute(&client, install(worktree, 1)).await);
        assert!(updated.settings.is_some());
        let mut context = Context {
            package: updated.summary.reference(),
            ..context
        };
        assert_eq!(read(&client, &context, "saved").await.revision, 1);
        let Output::PluginSettings(saved) = execute(
            &client,
            Command::SavePluginSettings {
                package: context.package.clone(),
                values: BTreeMap::from([("note".into(), json!("configured"))]),
                secrets: BTreeMap::new(),
            },
        )
        .await
        else {
            panic!("plugin settings expected")
        };
        assert_eq!(saved.package.settings_revision, 1);
        context.package = saved.package;
        assert_eq!(
            read(&client, &context, "saved").await.value,
            json!({"note":"kept"})
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: info(
                    execute(
                        &client,
                        Command::ReadPlugin {
                            name: "example".into(),
                        },
                    )
                    .await,
                )
                .summary
                .revision,
            },
        )
        .await;
        let reinstalled = info(execute(&client, install(worktree, 0)).await);
        context.package = reinstalled.summary.reference();
        assert_eq!(
            read(&client, &context, "saved").await.value,
            json!({"note":"kept"})
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
