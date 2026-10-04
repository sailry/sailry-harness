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

#[tokio::test]
async fn edits_and_restores() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let profile = root.join("node");
    let node = Node::start(&profile).await.unwrap();
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
        assert_eq!(
            client
                .execute(client.prepare(Command::RemoveProject {
                    expected: updated.clone()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        client
            .execute(client.prepare(Command::StopTurn { turn: turn.id }))
            .await
            .unwrap();
        let Output::Terminal(terminal) = client
            .execute(client.prepare(Command::CreateTerminal(terminal::Launch {
                worktree: session.worktree,
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
            })))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        assert_eq!(
            client
                .execute(client.prepare(Command::RemoveProject {
                    expected: updated.clone()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        client
            .execute(client.prepare(Command::CloseTerminal {
                worktree: session.worktree,
                terminal: terminal.id,
            }))
            .await
            .unwrap();
        let request = client.prepare(Command::RemoveProject {
            expected: updated.clone(),
        });
        let result = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), result);
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
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start(&profile).await.unwrap();
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
