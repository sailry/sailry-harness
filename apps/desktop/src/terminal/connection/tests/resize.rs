use super::*;

fn viewport(columns: u16) -> Viewport {
    Viewport {
        columns,
        rows: 24,
        pixel_width: u32::from(columns) * 8,
        pixel_height: 384,
    }
}

#[tokio::test]
#[cfg(unix)]
async fn resends_failed_and_coalesces_pending_geometry() {
    let directory = tempfile::tempdir().unwrap();
    let local = Node::start_isolated(directory.path().join("local"))
        .await
        .unwrap();
    let remote = Node::start_isolated(directory.path().join("remote"))
        .await
        .unwrap();
    let address = local
        .link()
        .pair(remote.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let color = terminal::Rgb {
        red: 128,
        green: 128,
        blue: 128,
    };
    let appearance = Appearance {
        foreground: color,
        background: color,
        palette: [color; 16],
        color_scheme: terminal::ColorScheme::Dark,
    };
    for (lane, transport) in [
        local.local() as Arc<dyn Transport>,
        local.link().remote(address),
    ]
    .into_iter()
    .enumerate()
    {
        let controller = Client::new(transport.clone());
        let root = directory.path().join(format!("project-{lane}"));
        std::fs::create_dir(&root).unwrap();
        let Output::Project(project) = controller
            .execute(controller.prepare(Command::RegisterProject {
                name: "Terminal geometry".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = controller
            .execute(controller.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.project == Some(project.id))
            .unwrap()
            .id;
        for queued in [false, true] {
            let Output::Terminal(info) = controller
                .execute(
                    controller.prepare(Command::CreateTerminal(terminal::Launch {
                        worktree,
                        access: sailry_protocol::process::Access::Workspace,
                        viewport: viewport(80),
                        appearance: appearance.clone(),
                    })),
                )
                .await
                .unwrap()
            else {
                panic!("terminal expected")
            };
            let held = Arc::new(Held {
                inner: transport.clone(),
                entered: CancellationToken::new(),
                release: CancellationToken::new(),
                screen: CancellationToken::new(),
                delay_close: false,
                reject_resize: AtomicBool::new(true),
                commands: Mutex::new(Vec::new()),
            });
            let binding = Binding {
                client: Arc::new(Client::new(held.clone())),
                caller: local.id(),
                id: info.id,
                runtime: tokio::runtime::Handle::current(),
                scope: None,
            };
            let (connection, mut screens, mut results) =
                Connection::start(&binding, appearance.clone());
            tokio::time::timeout(Duration::from_secs(10), async {
                while screens.borrow().snapshot.is_none() {
                    screens.changed().await.unwrap();
                }
            })
            .await
            .unwrap();
            let revision = screens.borrow().snapshot.as_ref().unwrap().info.revision;
            let resize = |columns| Command::ResizeTerminal {
                terminal: info.id,
                revision,
                viewport: viewport(columns),
            };
            assert!(connection.enqueue(resize(80)));
            tokio::time::timeout(Duration::from_secs(10), held.entered.cancelled())
                .await
                .unwrap();
            if queued {
                for columns in (40..80).rev() {
                    assert!(connection.enqueue(resize(columns)));
                    if columns == 60 {
                        assert!(connection.enqueue(Command::InputTerminal {
                            terminal: info.id,
                            revision,
                            input: terminal::Input::Text {
                                text: "discarded".into()
                            },
                        }));
                    }
                }
            }
            held.release.cancel();
            let failure = tokio::time::timeout(Duration::from_secs(10), results.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(failure.unwrap_err().code, ErrorCode::Unavailable);
            if !queued {
                assert!(connection.enqueue(resize(80)));
            }
            tokio::time::timeout(Duration::from_secs(10), results.recv())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(
                *held.commands.lock().unwrap(),
                vec![resize(80), resize(if queued { 40 } else { 80 })]
            );
            let Output::TerminalSnapshot(snapshot) = controller
                .execute(controller.prepare(Command::InspectTerminal { terminal: info.id }))
                .await
                .unwrap()
            else {
                panic!("terminal snapshot expected")
            };
            assert_eq!(snapshot.screen.columns, if queued { 40 } else { 80 });
            drop(connection);
            controller
                .execute(controller.prepare(Command::CloseTerminal {
                    worktree,
                    terminal: info.id,
                }))
                .await
                .unwrap();
        }
    }
    local.shutdown().await.unwrap();
    remote.shutdown().await.unwrap();
}
