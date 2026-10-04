use super::*;
use sailry_protocol::{
    dispatch as jobs,
    plugin::{Action, Context},
};
use serde_json::json;

#[path = "host/versions.rs"]
mod versions;

async fn installed(
    client: &Client,
    root: &Path,
    worktree: WorktreeId,
    source: &str,
    revision: u64,
) -> Context {
    fs::create_dir_all(root.join(sailry_protocol::plugin::NAMESPACE)).unwrap();
    let path = root.join("plugin.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["extensions"] = json!({"dev.sailry.platform": {
        "api_version":"v1", "actions":[Action::ReadStorage, Action::WriteStorage, Action::Notify, Action::Dispatch],
        "host":{"entry":"dev.sailry.platform/main.js", "resources":["dev.sailry.platform/main.js"], "handlers":["run"]}
    }});
    if root.join("dev.sailry.platform/settings.json").exists() {
        manifest["extensions"]["dev.sailry.platform"]["settings_schema"] =
            json!("dev.sailry.platform/settings.json");
    }
    fs::write(path, manifest.to_string()).unwrap();
    fs::write(root.join("dev.sailry.platform/main.js"), source).unwrap();
    let package = info(execute(client, install(worktree, revision)).await);
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: package.summary.reference(),
        worktree: Some(worktree),
        session: None,
    }
}

fn call(client: &Client, context: &Context, input: serde_json::Value) -> Request {
    client
        .prepare(Command::CallPlugin {
            handler: "run".into(),
            input,
        })
        .with_plugin(context.clone())
}

async fn read(client: &Client, context: &Context, key: &str) -> plugin::storage::Entry {
    let Output::PluginValue(value) = client
        .execute(
            client
                .prepare(Command::ReadPluginValue { key: key.into() })
                .with_plugin(context.clone()),
        )
        .await
        .unwrap()
    else {
        panic!("value expected")
    };
    value
}

const COUNTER: &str = r#"
import {getValue, setValue, completeRequest, forgetRequest, prepareNotification} from 'sailry/sdk';
export async function run(input) {
    const old = await getValue('counter');
    const id = setValue('counter', (old.value ?? 0) + input.amount, old.revision);
    const result = await completeRequest(id);
    if (result.Err) return result;
    forgetRequest(id);
    const notice = prepareNotification({title:'Counter',message:'Updated',kind:'success',session:null});
    const delivered = await completeRequest(notice);
    if (delivered.Err) return delivered;
    forgetRequest(notice);
    return result.Ok.data;
}
"#;

#[tokio::test]
async fn cancellation_keeps_node_running() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = installed(
            &client,
            &directory.path().join("source/package"),
            worktree,
            r#"
            import {setValue, completeRequest} from 'sailry/sdk';
            export async function run(input) {
                await completeRequest(setValue(input.key, true, '0'));
                if (input.wait) while (true) {}
                return input.key;
            }
        "#,
            0,
        )
        .await;
        let first = call(&client, &context, json!({"key":"first","wait":true}));
        let admitted = client.dispatch(first.clone()).await.unwrap();
        let second = call(&client, &context, json!({"key":"second","wait":true}));
        let other = client.dispatch(second.clone()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !read(&client, &context, "first").await.present
                || !read(&client, &context, "second").await.present
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let mut waiting = Vec::new();
        for index in 0..30 {
            let request = call(&client, &context, json!({"key":format!("queued-{index}")}));
            let pending = client.dispatch(request.clone()).await.unwrap();
            assert!(pending.receipt.durable);
            waiting.push((request, pending));
        }
        assert_eq!(
            client
                .execute(call(&client, &context, json!({"key":"third"})))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        assert!(!read(&client, &context, "queued-0").await.present);
        let (cancelled, pending) = waiting.pop().unwrap();
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::CancelPluginCall {
                            request: cancelled.id
                        })
                        .with_plugin(context.clone())
                )
                .await
                .unwrap(),
            Output::PluginCallStopping {
                request: cancelled.id
            }
        );
        assert_eq!(
            pending.completion.await.unwrap().unwrap_err().code,
            ErrorCode::Cancelled
        );
        assert!(!read(&client, &context, "queued-29").await.present);
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        let stopping = client
            .execute(
                client
                    .prepare(Command::CancelPluginCall { request: first.id })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap();
        assert_eq!(stopping, Output::PluginCallStopping { request: first.id });
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), admitted.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        // Queued calls keep their admitted authority after disabling the package.
        for (request, pending) in waiting {
            let result = tokio::time::timeout(Duration::from_secs(10), pending.completion)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(matches!(result, Output::PluginResult(_)));
            assert_eq!(client.execute(request).await.unwrap(), result);
        }
        execute(&client, Command::Snapshot).await;
        drop(other);
        tokio::time::timeout(Duration::from_secs(10), node.shutdown())
            .await
            .unwrap()
            .unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            client.execute(first).await.unwrap_err().code,
            ErrorCode::Cancelled
        );
        assert_eq!(
            client.execute(second).await.unwrap_err().code,
            ErrorCode::Cancelled
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn executes_the_external_summary_package() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/summary");
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/project-summary");
        for file in [
            "plugin.json",
            "dev.sailry.platform/settings.json",
            "dev.sailry.platform/host/main.js",
            "dev.sailry.platform/desktop/main.js",
            "dev.sailry.platform/desktop/report.js",
            "dev.sailry.platform/desktop/locales.js",
        ] {
            let target = root.join(file);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(source.join(file), target).unwrap();
        }
        git2::Repository::init(directory.path().join("source")).unwrap();
        fs::write(directory.path().join("source/notes.txt"), "Notes").unwrap();
        let package = info(
            execute(
                &client,
                Command::InstallPlugin {
                    worktree,
                    path: "summary".into(),
                    name: "project-summary".into(),
                    expected_revision: 0,
                },
            )
            .await,
        );
        assert!(package.issues.is_empty(), "{:?}", package.issues);
        let context = Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(worktree),
            session: None,
        };
        let request = client
            .prepare(Command::CallPlugin {
                handler: "save".into(),
                input: json!({"locale":"zh-CN","include_untracked":true,"expected_revision":null}),
            })
            .with_plugin(context.clone());
        let result = client.execute(request.clone()).await.unwrap();
        let Output::PluginResult(ref result_json) = result else {
            panic!()
        };
        assert_eq!(result_json["Ok"]["kind"], "file_written", "{result_json}");
        let path = directory.path().join("source/project-summary.md");
        let report = fs::read_to_string(&path).unwrap();
        assert!(report.contains("notes.txt"));
        assert!(report.starts_with("# 项目变更"));
        fs::write(&path, "External replacement").unwrap();
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert_eq!(fs::read_to_string(path).unwrap(), "External replacement");
        let mut tampered = client
            .prepare(Command::CallPlugin {
                handler: "undeclared".into(),
                input: json!(null),
            })
            .with_plugin(context.clone());
        assert_eq!(
            client.execute(tampered.clone()).await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        tampered.id = RequestId::new();
        tampered.command = Command::CallPlugin {
            handler: "summarize".into(),
            input: json!(null),
        };
        fs::write(
            directory
                .path()
                .join("node/plugins/packages")
                .join(&context.package.digest)
                .join("dev.sailry.platform/host/main.js"),
            "export function summarize() { return 'tampered'; }",
        )
        .unwrap();
        assert_eq!(
            client.execute(tampered).await.unwrap_err().code,
            ErrorCode::Unavailable
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn executes_without_a_view_and_recovers_receipts() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = installed(
            &client,
            &directory.path().join("source/package"),
            worktree,
            COUNTER,
            0,
        )
        .await;
        let request = call(&client, &context, json!({"amount":3}));
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let result = admitted.completion.await.unwrap().unwrap();
        assert_eq!(
            result,
            Output::PluginResult(json!({"key":"counter","revision":"1","present":true,"value":3}))
        );
        assert_eq!(client.execute(request.clone()).await.unwrap(), result);
        assert_eq!(read(&client, &context, "counter").await.revision, 1);
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.notifications.len(), 1);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert_eq!(read(&client, &context, "counter").await.value, json!(3));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn interrupted_admission_is_not_replayed() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = installed(
            &client,
            &directory.path().join("source/package"),
            worktree,
            COUNTER,
            0,
        )
        .await;
        let request = call(&client, &context, json!({"amount":99}));
        let caller = if remote {
            NodeId(*controller.handle().address().id.as_bytes())
        } else {
            node.id()
        };
        drop(client);
        node.shutdown().await.unwrap();
        // An isolated crash fixture, never the user's running profile.
        let db =
            rusqlite::Connection::open(directory.path().join("node/storage/node.sqlite3")).unwrap();
        db.execute(
            "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
            rusqlite::params![
                &caller.0[..],
                request.id.to_string(),
                serde_json::to_vec(&request).unwrap()
            ],
        )
        .unwrap();
        drop(db);
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(!read(&client, &context, "counter").await.present);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn retains_active_version_and_rejects_forged_scopes() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        let settings = |label: &str| json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"properties":{"label":{"type":"string","default":label}}});
        fs::write(
            root.join("dev.sailry.platform/settings.json"),
            settings("admitted").to_string(),
        )
        .unwrap();
        let code = format!(r#"
import {{readSettings, context, prepareRequest}} from 'sailry/sdk';
{COUNTER}
export async function waitForRelease() {{}}
"#).replace("export async function run(input) {", r#"export async function run(input) {
    await completeRequest(setValue('started', true, '0'));
    while (!(await getValue('release')).value) { const until = Date.now() + 10; while (Date.now() < until) {} }
    if ((await readSettings()).values.label !== 'admitted') throw new Error('Settings version changed');
    const query = prepareRequest({kind:'dispatch',data:{package:context().package,action:{kind:'list_handlers'}}});
    if ((await completeRequest(query)).Err) throw new Error('Original dispatch scope lost');
    forgetRequest(query);
"#);
        let context = installed(&client, &root, worktree, &code, 0).await;
        let request = call(&client, &context, json!({"amount":5}));
        let admitted = client.dispatch(request.clone()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !read(&client, &context, "started").await.present {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        fs::write(
            root.join("dev.sailry.platform/settings.json"),
            settings("replacement").to_string(),
        )
        .unwrap();
        let next = installed(
            &client,
            &root,
            worktree,
            "export function run() {return 'replacement';}",
            1,
        )
        .await;
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: 2,
                enabled: false,
            },
        )
        .await;
        assert!(
            client
                .execute(call(&client, &next, json!(null)))
                .await
                .is_err()
        );
        execute(
            &client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: 3,
            },
        )
        .await;
        let mut active = context.clone();
        active.invocation = Some(request.id);
        let mut forged = active.clone();
        forged.worktree = Some(WorktreeId::new());
        assert!(
            client
                .execute(
                    client
                        .prepare(Command::ReadPluginValue {
                            key: "counter".into()
                        })
                        .with_plugin(forged)
                )
                .await
                .is_err()
        );
        // The authenticated caller and original scope must also match the parent.
        let other = Client::new(if remote {
            node.local()
        } else {
            controller.handle().remote(node.link().address())
        });
        assert_eq!(
            other
                .execute(
                    other
                        .prepare(Command::ReadPluginValue {
                            key: "counter".into()
                        })
                        .with_plugin(active.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        client
            .execute(
                client
                    .prepare(Command::WritePluginValue {
                        key: "release".into(),
                        value: json!(true),
                        expected_revision: 0,
                    })
                    .with_plugin(active.clone()),
            )
            .await
            .unwrap();
        let result = admitted.completion.await.unwrap().unwrap();
        assert_eq!(
            result,
            Output::PluginResult(json!({"key":"counter","revision":"1","present":true,"value":5}))
        );
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ReadPluginValue {
                            key: "counter".into()
                        })
                        .with_plugin(active)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!()
        };
        assert_eq!(snapshot.notifications.len(), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn scheduled_callbacks_survive_restart() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = installed(
            &client,
            &directory.path().join("source/package"),
            worktree,
            COUNTER,
            0,
        )
        .await;
        dispatch::run(
            &client,
            &context,
            jobs::Command::SaveHandler(jobs::Handler {
                name: "receive".into(),
                revision: 0,
                enabled: true,
                source: jobs::Source {
                    package: "example".into(),
                    topic: "tick".into(),
                },
                queue: "background".into(),
                callback: jobs::Callback {
                    scope: jobs::Scope {
                        worktree: Some(worktree),
                        session: None,
                    },
                    completion: jobs::Completion::Command,
                    command: Box::new(Command::CallPlugin {
                        handler: "run".into(),
                        input: json!(null),
                    }),
                    bindings: std::collections::BTreeMap::from([(
                        "/data/input".into(),
                        "/payload".into(),
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
                enabled: false,
                topic: "tick".into(),
                payload: json!({"amount":7}),
                timing: jobs::Timing::Once {
                    at_ms: chrono::Utc::now().timestamp_millis() + 100,
                },
                next_ms: None,
            }),
        )
        .await;
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let jobs::Output::Schedules(mut schedules) =
            dispatch::run(&client, &context, jobs::Command::ListSchedules).await
        else {
            panic!("schedules expected")
        };
        assert_eq!(schedules.len(), 1);
        let mut schedule = schedules.pop().unwrap();
        assert!(!schedule.enabled);
        schedule.enabled = true;
        dispatch::run(&client, &context, jobs::Command::SaveSchedule(schedule)).await;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let jobs::Output::Jobs(page) = dispatch::run(
                    &client,
                    &context,
                    jobs::Command::ListJobs {
                        before: None,
                        limit: 10,
                    },
                )
                .await
                else {
                    panic!()
                };
                if let Some(job) = page.jobs.first()
                    && !matches!(job.status, jobs::Status::Queued | jobs::Status::Running)
                {
                    assert_eq!(job.status, jobs::Status::Completed, "{:?}", job.error);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(read(&client, &context, "counter").await.value, json!(7));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
