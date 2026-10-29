use super::fixture::Fixture;
use super::*;
use sailry_protocol::plugin;

mod dispatch;
mod hooks;
mod output;
mod permissions;

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

async fn select(fixture: &mut Fixture, fixed: bool, dispatch: bool) -> role::Profile {
    let mut role = fixture
        .snapshot()
        .await
        .roles
        .into_iter()
        .find(|role| role.key == "explore")
        .unwrap();
    if fixed {
        role.model = fixture.role.model.clone();
        let mut tools = vec![plugin::conversation::Tool::Package {
            package: "files".into(),
            name: "read_file".into(),
        }];
        if dispatch {
            tools.push(plugin::conversation::Tool::Package {
                package: dispatch::PACKAGE.into(),
                name: "inspect".into(),
            });
        }
        role.tools = Some(tools);
        let revision = role.revision;
        let Output::Role(saved) = execute(
            &fixture.client,
            Command::PutRole {
                role,
                expected_revision: revision,
            },
        )
        .await
        else {
            panic!("role expected")
        };
        role = saved;
    }
    let mut config = fixture.session.config.clone();
    config.mode = WorkMode::Code;
    config.permission = Permission::Full;
    let Output::Session(session) = execute(
        &fixture.client,
        Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        },
    )
    .await
    else {
        panic!("session expected")
    };
    let Output::Session(session) = execute(
        &fixture.client,
        Command::SetSessionRoles {
            session: session.id,
            expected_revision: session.revision,
            roles: vec![role.reference()],
        },
    )
    .await
    else {
        panic!("session expected")
    };
    fixture.session = session;
    role
}

#[tokio::test]
async fn inherits_model_without_write_authority() {
    for remote in [false, true] {
        let parent = Server::parallel(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"explore","task":"Inspect the existing project"}),
        )])
        .await;
        let unused = Server::start(false).await;
        let mut fixture = Fixture::start(remote, parent, unused).await;
        let role = select(&mut fixture, false, false).await;
        assert!(role.model.is_none());
        let turn = fixture.push("Delegate the exploration").await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let output = admissions(&fixture.client, &page).await;
        assert_eq!(output.len(), 1);
        completed(
            &fixture.client,
            &page,
            &fixture.parent,
            &output[0],
            "answer-fixture-a",
        )
        .await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        let mut expected = turn.config;
        expected.mode = WorkMode::Plan;
        assert_eq!(child.config, expected);
        assert_eq!(child.delegation.as_ref().unwrap().role, Some(role.id));
        assert!(fixture.child.requests.lock().unwrap().is_empty());
        let mut changed = child.config.clone();
        changed.mode = WorkMode::Code;
        let fault = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: child.id,
                expected_revision: child.revision,
                config: changed.clone(),
            }))
            .await
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::PermissionDenied);
        changed.mode = WorkMode::Plan;
        changed.effort = Effort::High;
        changed.permission = Permission::Project;
        let fault = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: child.id,
                expected_revision: child.revision,
                config: changed,
            }))
            .await
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::PermissionDenied);
        let Output::Session(retained) =
            execute(&fixture.client, Command::ReadSession { session: child.id }).await
        else {
            panic!("session expected")
        };
        assert_eq!(retained, child);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn reuses_frozen_read_only_role() {
    for remote in [false, true] {
        let parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"explore","task":"Read source.txt and inspect private state"}),
        )])
        .await;
        let child = Server::turn_tools(vec![
            (
                plugin_tool("files", "read_file"),
                json!({"path":"source.txt"}),
            ),
            (plugin_tool(dispatch::PACKAGE, "inspect"), json!({})),
        ])
        .await;
        let mut fixture = Fixture::start(remote, parent, child).await;
        std::fs::write(fixture.root().join("source.txt"), dispatch::EVIDENCE).unwrap();
        let package = dispatch::install(&fixture).await;
        let role = select(&mut fixture, true, true).await;
        let first = fixture
            .push("Inspect the project without changing it")
            .await;
        let page = finished(&fixture.client, fixture.session.id, first.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let output = admissions(&fixture.client, &page).await;
        assert_eq!(output.len(), 1);
        completed(
            &fixture.client,
            &page,
            &fixture.parent,
            &output[0],
            "answer-fixture-child",
        )
        .await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        dispatch::verify(&fixture, &child, &package).await;
        let mut edited = role.clone();
        edited.key = "writable-explorer".into();
        edited.read_only = false;
        edited.instructions = "Changed role instructions".into();
        let Output::Role(edited) = execute(
            &fixture.client,
            Command::PutRole {
                role: edited,
                expected_revision: role.revision,
            },
        )
        .await
        else {
            panic!("role expected")
        };
        execute(
            &fixture.client,
            Command::RemoveRole {
                role: edited.id,
                expected_revision: edited.revision,
            },
        )
        .await;
        let reused = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"session":child.id,"task":"Read source.txt and inspect private state again"}),
        )])
        .await;
        fixture.route_parent(&reused.endpoint).await;
        let second = fixture.push("Continue the exploration").await;
        let page = finished(&fixture.client, fixture.session.id, second.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let output = admissions(&fixture.client, &page).await;
        assert_eq!(output.len(), 2);
        completed(
            &fixture.client,
            &page,
            &reused,
            output.last().unwrap(),
            "answer-fixture-child",
        )
        .await;
        let retained = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        assert_eq!(retained.id, child.id);
        assert_eq!(retained.config, child.config);
        dispatch::verify(&fixture, &retained, &package).await;
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 6);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_writable_child_configuration() {
    for remote in [false, true] {
        let parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","task":"Inspect the project"}),
        )])
        .await;
        let child = Server::start(false).await;
        let mut fixture = Fixture::start(remote, parent, child).await;
        let mut provider = fixture.provider.clone();
        provider.models.push(model("fixture-alternate"));
        execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await;
        let Output::Session(session) = execute(
            &fixture.client,
            Command::SetSessionRoles {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                roles: vec![fixture.role.reference()],
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let turn = fixture.push("Delegate the inspection").await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        assert_eq!(child.config.mode, WorkMode::Code);
        let mut config = child.config.clone();
        config.model = "fixture-alternate".into();
        config.effort = Effort::High;
        config.permission = Permission::Project;
        let fault = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: child.id,
                expected_revision: child.revision,
                config: config.clone(),
            }))
            .await
            .unwrap_err();
        assert_eq!(fault.code, ErrorCode::PermissionDenied);
        for mode in [WorkMode::Plan, WorkMode::Code] {
            let mut config = child.config.clone();
            config.mode = mode;
            let fault = fixture
                .client
                .execute(fixture.client.prepare(Command::SetSessionConfig {
                    session: child.id,
                    expected_revision: child.revision,
                    config,
                }))
                .await
                .unwrap_err();
            assert_eq!(fault.code, ErrorCode::PermissionDenied);
        }
        let Output::Session(retained) =
            execute(&fixture.client, Command::ReadSession { session: child.id }).await
        else {
            panic!("session expected")
        };
        assert_eq!(retained, child);
        for mode in [WorkMode::Plan, WorkMode::Code] {
            let mut config = fixture.session.config.clone();
            config.mode = mode;
            let Output::Session(saved) = execute(
                &fixture.client,
                Command::SetSessionConfig {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    config,
                },
            )
            .await
            else {
                panic!("session expected")
            };
            assert_eq!(saved.config.mode, mode);
            fixture.session = saved;
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
