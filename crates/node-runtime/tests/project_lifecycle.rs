use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;

async fn snapshot(client: &Client) -> Snapshot {
    let Output::Snapshot(value) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    value
}

fn launch(worktree: WorktreeId) -> terminal::Launch {
    terminal::Launch {
        access: sailry_protocol::process::Access::Workspace,
        worktree,
        viewport: terminal::Viewport {
            columns: 80,
            rows: 24,
            pixel_width: 640,
            pixel_height: 480,
        },
        appearance: terminal::Appearance {
            foreground: terminal::Rgb {
                red: 240,
                green: 240,
                blue: 240,
            },
            background: terminal::Rgb {
                red: 20,
                green: 20,
                blue: 20,
            },
            palette: [terminal::Rgb {
                red: 128,
                green: 128,
                blue: 128,
            }; 16],
            color_scheme: terminal::ColorScheme::Dark,
        },
    }
}

#[tokio::test]
async fn edits_and_restores() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let profile = root.join("node");
    let node = Node::start_empty(&profile).await.unwrap();
    let controller = Link::controller(root.join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let mut removed = Vec::new();
    for (index, transport) in [node.local(), controller.handle().remote(address)]
        .into_iter()
        .enumerate()
    {
        let client = Client::new(transport);
        let path = root.join(format!("project-{index}"));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("keep.txt"), "preserved").unwrap();
        let Output::Project(original) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Original".into(),
                path: path.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let request = client.prepare(Command::UpdateProject {
            expected: original.clone(),
            name: "Renamed".into(),
            path: original.path.clone(),
            appearance: projects::Appearance {
                icon: "code".into(),
                color: "violet".into(),
            },
        });
        let result = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), result);
        let Output::Project(updated) = result else {
            panic!("project expected")
        };
        assert_eq!(
            snapshot(&client)
                .await
                .projects
                .iter()
                .find(|project| project.id == original.id),
            Some(&updated)
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::RemoveProject { expected: original }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: Some(updated.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider: ProviderId::new(),
                    model: "fixture".into(),
                    effort: Effort::High,
                    mode: WorkMode::Code,
                    options: Default::default(),
                    permission: Permission::Ask,
                    credential: None,
                }),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(
            client
                .execute(client.prepare(Command::UpdateProject {
                    expected: updated.clone(),
                    name: "Move".into(),
                    path: root.to_str().unwrap().into(),
                    appearance: updated.appearance.clone()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Held fixture task".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        let busy_request = client.prepare(Command::RemoveProject {
            expected: updated.clone(),
        });
        let busy = client.execute(busy_request.clone()).await.unwrap_err();
        assert_eq!(busy.code, ErrorCode::Busy);
        assert_eq!(
            busy.details,
            Some(FaultDetails::ProjectRemoval {
                tasks_active: true,
                terminals_may_be_closed: false,
            })
        );
        assert_eq!(
            client.execute(busy_request.clone()).await.unwrap_err(),
            busy
        );
        assert_eq!(
            client.outcome(&busy_request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Err(busy)))
        );
        client
            .execute(client.prepare(Command::StopTurn { turn: turn.id }))
            .await
            .unwrap();
        let Output::Terminal(terminal) = client
            .execute(client.prepare(Command::CreateTerminal(launch(session.worktree))))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        assert_eq!(terminal.status, terminal::Status::Running);
        let other_path = root.join(format!("other-{index}"));
        std::fs::create_dir(&other_path).unwrap();
        let Output::Project(other) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Other project".into(),
                path: other_path.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let other_tree = snapshot(&client)
            .await
            .worktrees
            .into_iter()
            .find(|tree| tree.project == Some(other.id))
            .unwrap()
            .id;
        let Output::Terminal(other_terminal) = client
            .execute(client.prepare(Command::CreateTerminal(launch(other_tree))))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let history = client
            .read_conversation(session.id, None, 100)
            .await
            .unwrap();
        let retained_session = snapshot(&client)
            .await
            .sessions
            .into_iter()
            .find(|value| value.id == session.id)
            .unwrap();
        let request = client.prepare(Command::RemoveProject {
            expected: updated.clone(),
        });
        let result = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert_eq!(result, Output::ProjectRemoved { id: updated.id });
        let state = snapshot(&client).await;
        let closed = state
            .terminals
            .iter()
            .find(|info| info.id == terminal.id)
            .unwrap();
        assert_eq!(closed.status, terminal::Status::Closed);
        assert_eq!(closed.owner, None);
        assert!(closed.revision > terminal.revision);
        assert_eq!(
            client
                .execute(client.prepare(Command::InspectTerminal {
                    terminal: terminal.id
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
        );
        assert_eq!(
            state
                .terminals
                .iter()
                .find(|info| info.id == other_terminal.id)
                .unwrap()
                .status,
            terminal::Status::Running,
        );
        assert!(matches!(
            client
                .execute(client.prepare(Command::InspectTerminal {
                    terminal: other_terminal.id
                }))
                .await
                .unwrap(),
            Output::TerminalSnapshot(_),
        ));
        assert!(
            state
                .sessions
                .iter()
                .any(|value| value == &retained_session)
        );
        assert_eq!(
            client
                .read_conversation(session.id, None, 100)
                .await
                .unwrap(),
            history
        );
        assert!(
            !snapshot(&client)
                .await
                .projects
                .iter()
                .any(|project| project.id == updated.id)
        );
        assert_eq!(
            std::fs::read_to_string(path.join("keep.txt")).unwrap(),
            "preserved"
        );
        let Output::Project(restored) = client
            .execute(client.prepare(Command::CreateProject(projects::Draft {
                name: updated.name.clone(),
                path: updated.path.clone(),
                appearance: updated.appearance.clone(),
                source: projects::Source::Local,
            })))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        assert_eq!(restored, updated);
        assert!(
            snapshot(&client)
                .await
                .sessions
                .iter()
                .any(|value| value.id == session.id && value.worktree == session.worktree)
        );
        client
            .execute(client.prepare(Command::RemoveProject {
                expected: restored.clone(),
            }))
            .await
            .unwrap();
        removed.push(restored);
        client
            .execute(client.prepare(Command::RemoveProject { expected: other }))
            .await
            .unwrap();
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start_empty(&profile).await.unwrap();
    let client = Client::new(node.local());
    assert!(snapshot(&client).await.projects.is_empty());
    for project in removed {
        let Output::Project(restored) = client
            .execute(client.prepare(Command::RegisterProject {
                name: project.name.clone(),
                path: project.path.clone(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        assert_eq!(restored.id, project.id);
    }
    node.shutdown().await.unwrap();
}
