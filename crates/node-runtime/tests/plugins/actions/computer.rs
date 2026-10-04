//! The same native tool request crosses local and authenticated IROH boundaries.
#![cfg(unix)]
use super::*;
use serde_json::{Value, json};
#[path = "../../support/computer.rs"]
mod driver;
#[cfg(target_os = "macos")]
#[path = "computer/native.rs"]
mod native;

fn request(client: &Client, context: &Context, name: &str, arguments: Value) -> Request {
    client
        .prepare(Command::UseComputer {
            session: context.session.unwrap(),
            worktree: context.worktree.unwrap(),
            name: name.into(),
            arguments,
        })
        .with_plugin(context.clone())
}

struct Fixture {
    directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    client: Client,
    context: Context,
    worker: driver::Worker,
}

impl Fixture {
    async fn new(remote: bool, actions: &[Action], outcomes: Value) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let worker = driver::Worker::new(directory.path(), outcomes);
        let node = Node::start_with_computer(
            directory.path().join("node"),
            NetworkScope::default(),
            Some(worker.configuration.clone()),
        )
        .await
        .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let root = directory.path().join("source");
        package(&root.join("package"), "1.0.0");
        execute(
            &client,
            Command::RegisterProject {
                name: "Computer fixture".into(),
                path: root.to_str().unwrap().into(),
            },
        )
        .await;
        let Output::Snapshot(snapshot) = execute(&client, Command::Snapshot).await else {
            panic!("snapshot expected")
        };
        let worktree = snapshot.worktrees[0].id;
        let mut context =
            install_actions(&client, &root.join("package"), worktree, 0, actions).await;
        context.session = Some(scopes::session(&client, worktree).await.id);
        Self {
            directory,
            node,
            controller,
            client,
            context,
            worker,
        }
    }

    async fn shutdown(self) {
        drop(self.client);
        self.controller.close().await.unwrap();
        self.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_raw_dispatch_and_recovers_receipts() {
    let arguments = json!({"target":{"kind":"window","pid":314,"window_id":271},"x":12,"y":34,"delivery_mode":"background"});
    let mut outcome = driver::action();
    outcome["_meta"] = json!({"extension":{"marker":7}});
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            &[Action::ReadComputer, Action::ControlComputer],
            json!({"click":outcome}),
        )
        .await;
        let original = request(
            &fixture.client,
            &fixture.context,
            "click",
            arguments.clone(),
        );
        let pending = fixture.client.dispatch(original.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        drop(pending);
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let RequestOutcome::Completed(result) =
                    fixture.client.outcome(&original).await.unwrap()
                {
                    break result.unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(result, Output::Computer(outcome.clone()));
        assert_eq!(fixture.worker.calls().len(), 1);
        assert_eq!(fixture.worker.calls()[0]["arguments"], arguments);
        let mut changed = original.clone();
        let Command::UseComputer { arguments, .. } = &mut changed.command else {
            unreachable!()
        };
        arguments["x"] = json!(99);
        assert_eq!(
            fixture.client.execute(changed).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        let disabled = info(
            execute(
                &fixture.client,
                Command::SetPluginEnabled {
                    name: "example".into(),
                    expected_revision: 1,
                    enabled: false,
                },
            )
            .await,
        );
        assert_eq!(
            fixture.client.execute(original.clone()).await.unwrap(),
            result
        );
        assert_eq!(
            fixture.worker.calls().len(),
            1,
            "receipt recovery must not repeat native input"
        );
        assert_eq!(
            fixture
                .client
                .execute(request(
                    &fixture.client,
                    &fixture.context,
                    "click",
                    json!({"target":{"kind":"window","pid":314,"window_id":271},"delivery_mode":"background","x":12,"y":34})
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        execute(
            &fixture.client,
            Command::RemovePlugin {
                name: "example".into(),
                expected_revision: disabled.summary.revision,
            },
        )
        .await;
        let Fixture {
            directory,
            node,
            controller,
            client,
            context: _,
            worker,
        } = fixture;
        drop(client);
        node.shutdown().await.unwrap();
        let calls = worker.calls().len();
        let node = Node::start_with_computer(
            directory.path().join("node"),
            NetworkScope::default(),
            Some(worker.configuration.clone()),
        )
        .await
        .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(client.execute(original).await.unwrap(), result);
        assert_eq!(
            worker.calls().len(),
            calls,
            "restart recovery must not start another native call"
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn isolates_bound_session_handles() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote, &[Action::ReadComputer], json!({})).await;
        let second = scopes::session(&fixture.client, fixture.context.worktree.unwrap()).await;
        let mut other = fixture.context.clone();
        other.session = Some(second.id);
        for context in [&fixture.context, &other, &fixture.context] {
            fixture
                .client
                .execute(request(&fixture.client, context, "list_windows", json!({})))
                .await
                .unwrap();
        }
        let calls = fixture.worker.calls();
        assert_eq!(calls.len(), 3);
        assert!(calls[0]["session_handle"].is_string());
        assert_ne!(calls[0]["session_handle"], calls[1]["session_handle"]);
        assert_eq!(calls[0]["session_handle"], calls[2]["session_handle"]);
        let bound: Vec<_> = fixture
            .worker
            .records()
            .into_iter()
            .filter(|record| record["operation"] == "bind_session")
            .collect();
        assert_eq!(bound.len(), 2);
        for binding in &bound {
            let label = binding["arguments"]["public_session"].as_str().unwrap();
            assert!(uuid::Uuid::parse_str(label).is_ok());
        }
        assert_ne!(
            bound[0]["arguments"]["public_session"],
            bound[1]["arguments"]["public_session"]
        );
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn rejects_unpaired_native_input() {
    let fixture = Fixture::new(true, &[Action::ControlComputer], json!({})).await;
    let foreign = Link::controller(
        fixture.directory.path().join("unpaired-controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let client = Client::new(foreign.handle().remote(fixture.node.link().address()));
    let input = request(
        &client,
        &fixture.context,
        "click",
        json!({"target":{"kind":"window","pid":314,"window_id":271},"delivery_mode":"background","x":12,"y":34}),
    );
    assert_eq!(
        client.execute(input.clone()).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    foreign
        .handle()
        .set_trust(fixture.node.id(), true)
        .await
        .unwrap();
    assert!(
        client.execute(input).await.is_err(),
        "outgoing trust must not authorize an unknown caller on the execution Node"
    );
    assert!(fixture.worker.records().is_empty());
    drop(client);
    foreign.close().await.unwrap();
    fixture.shutdown().await;
}

#[tokio::test]
async fn uses_catalog_authority_and_resource_binding() {
    assert!(
        driver::catalog()
            .iter()
            .any(|tool| tool["name"] == "list_windows"
                && tool["annotations"]["readOnlyHint"] == true)
    );
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            &[Action::ReadComputer],
            json!({"list_windows":driver::result(json!({"windows":[]}))}),
        )
        .await;
        assert_eq!(
            fixture
                .client
                .execute(request(
                    &fixture.client,
                    &fixture.context,
                    "click",
                    json!({"target":{"kind":"window","pid":314,"window_id":271},"delivery_mode":"background","x":12,"y":34,"readOnlyHint":true})
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied,
            "caller metadata must not grant input authority"
        );
        assert!(fixture.worker.calls().is_empty());
        let result = fixture
            .client
            .execute(request(
                &fixture.client,
                &fixture.context,
                "list_windows",
                json!({"on_screen_only":false}),
            ))
            .await
            .unwrap();
        assert_eq!(
            result,
            Output::Computer(driver::result(json!({"windows":[]})))
        );
        assert_eq!(fixture.worker.calls().len(), 1);
        for field in ["session", "worktree"] {
            let mut outside = request(&fixture.client, &fixture.context, "list_windows", json!({}));
            let Command::UseComputer {
                session, worktree, ..
            } = &mut outside.command
            else {
                unreachable!()
            };
            if field == "session" {
                *session = SessionId::new();
            } else {
                *worktree = WorktreeId::new();
            }
            assert_eq!(
                fixture.client.execute(outside).await.unwrap_err().code,
                ErrorCode::PermissionDenied
            );
        }
        assert_eq!(
            fixture.worker.calls().len(),
            1,
            "foreign resource scopes must be refused before SDK dispatch"
        );
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn validates_only_native_identity_and_object_input() {
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            &[Action::ReadComputer, Action::ControlComputer],
            json!({}),
        )
        .await;
        for (name, arguments) in [
            ("computer_batch", json!({"actions":[]})),
            ("unknown_native_tool", json!({})),
            ("click", Value::Null),
        ] {
            let malformed = request(&fixture.client, &fixture.context, name, arguments);
            let fault = fixture.client.execute(malformed.clone()).await.unwrap_err();
            assert_eq!(fault.code, ErrorCode::InvalidRequest, "{name}: {fault:?}");
            assert_eq!(
                fixture.client.outcome(&malformed).await.unwrap(),
                RequestOutcome::Completed(Box::new(Err(fault)))
            );
        }
        assert!(fixture.worker.calls().is_empty());
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn preserves_sdk_errors_without_fallback_dispatch() {
    let outcome = json!({"content":[{"type":"text","text":"Unsupported accessibility action"}],"isError":true,
        "structuredContent":{"code":"ax_action_unsupported","message":"Unsupported accessibility action"}});
    let arguments = json!({"target":{"kind":"window","pid":314,"window_id":271},"delivery_mode":"background","element_token":"sdk-snapshot:12"});
    for remote in [false, true] {
        let fixture =
            Fixture::new(remote, &[Action::ControlComputer], json!({"click":outcome})).await;
        let original = request(
            &fixture.client,
            &fixture.context,
            "click",
            arguments.clone(),
        );
        assert_eq!(
            fixture.client.execute(original.clone()).await.unwrap(),
            Output::Computer(outcome.clone())
        );
        assert_eq!(
            fixture.client.execute(original).await.unwrap(),
            Output::Computer(outcome.clone())
        );
        let calls = fixture.worker.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["name"], "click");
        assert_eq!(calls[0]["arguments"], arguments);
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn keeps_software_actions_separate_from_agent_mode() {
    for remote in [false, true] {
        let fixture = Fixture::new(
            remote,
            &[Action::ReadComputer, Action::ControlComputer],
            json!({}),
        )
        .await;
        let session = scopes::session(&fixture.client, fixture.context.worktree.unwrap()).await;
        let mut config = session.config;
        config.mode = WorkMode::Plan;
        execute(
            &fixture.client,
            Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            },
        )
        .await;
        let mut context = fixture.context.clone();
        context.session = Some(session.id);
        // Work mode constrains admitted Agent turns. An explicitly declared
        // software action keeps its own grant and captured resource scope.
        fixture
            .client
            .execute(request(
                &fixture.client,
                &context,
                "click",
                json!({"target":{"kind":"window","pid":314,"window_id":271},"delivery_mode":"background","x":12,"y":34}),
            ))
            .await
            .unwrap();
        assert_eq!(fixture.worker.calls().len(), 1);
        fixture
            .client
            .execute(request(
                &fixture.client,
                &context,
                "list_windows",
                json!({}),
            ))
            .await
            .unwrap();
        assert_eq!(fixture.worker.calls().len(), 2);
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn reads_permissions_without_prompting() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _worktree) = super::fixture(remote).await;
        let package = info(
            execute(
                &client,
                Command::ReadPlugin {
                    name: "computer".into(),
                },
            )
            .await,
        );
        assert!(!package.summary.enabled);
        let mut context = Context {
            invocation: None,
            turn: None,
            surface: plugin::desktop::Surface::Settings,
            package: package.summary.reference(),
            worktree: None,
            session: None,
        };
        let Output::ComputerPermissions(permissions) = client
            .execute(
                client
                    .prepare(Command::ReadComputerPermissions)
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!("permissions expected")
        };
        assert_eq!(permissions.node, node.id());
        assert_eq!(permissions.local, !remote);
        if remote {
            assert_eq!(
                client
                    .execute(
                        client
                            .prepare(Command::RequestComputerPermission {
                                permission: sailry_protocol::computer::Permission::Accessibility,
                            })
                            .with_plugin(context.clone())
                    )
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        context.surface = Default::default();
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ReadComputerPermissions)
                        .with_plugin(context)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotConfigured
        );
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
