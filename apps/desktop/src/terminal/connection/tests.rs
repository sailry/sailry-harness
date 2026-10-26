use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{ErrorCode, Output, Request, Topic, Update, terminal};
use std::{sync::Mutex, time::Duration};

struct Held {
    inner: Arc<dyn Transport>,
    entered: CancellationToken,
    release: CancellationToken,
    screen: CancellationToken,
    delay_close: bool,
    commands: Mutex<Vec<Command>>,
}

impl Transport for Held {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn open(
        &self,
        id: sailry_protocol::StreamId,
    ) -> Pending<'_, Result<sailry_link::Stream, Fault>> {
        self.inner.open(id)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(
                request.command,
                Command::InputTerminal { .. }
                    | Command::ResizeTerminal { .. }
                    | Command::SetTerminalAppearance { .. }
            ) {
                self.commands.lock().unwrap().push(request.command.clone());
                if matches!(request.command, Command::InputTerminal { .. }) {
                    self.entered.cancel();
                    self.release.cancelled().await;
                }
            }
            self.inner.dispatch(request).await
        })
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            Ok(Box::new(Frames {
                inner: self.inner.subscribe(topic).await?,
                release: self.screen.clone(),
                delay: self.delay_close,
            }) as Box<dyn Subscription>)
        })
    }
}

struct Frames {
    inner: Box<dyn Subscription>,
    release: CancellationToken,
    delay: bool,
}
impl Subscription for Frames {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            let update = self.inner.next().await?;
            let closed = match &update {
                Update::TerminalSnapshot(snapshot) => {
                    snapshot.info.status == terminal::Status::Closed
                }
                Update::TerminalFrame(frame) => frame.info.status == terminal::Status::Closed,
                _ => false,
            };
            if closed && self.delay {
                self.release.cancelled().await;
            }
            Ok(update)
        })
    }
}

#[tokio::test]
#[cfg(unix)]
async fn respects_terminal_lifecycle() {
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
    let viewport = terminal::Viewport {
        columns: 80,
        rows: 24,
        pixel_width: 640,
        pixel_height: 384,
    };
    let color = terminal::Rgb {
        red: 128,
        green: 128,
        blue: 128,
    };
    let appearance = terminal::Appearance {
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
        for (case, (close, delay_close, release)) in [
            (true, false, false),
            (true, true, false),
            (false, false, false),
            (false, false, true),
        ]
        .into_iter()
        .enumerate()
        {
            let controller = Client::new(transport.clone());
            let root = directory.path().join(format!("project-{lane}-{case}"));
            std::fs::create_dir(&root).unwrap();
            let Output::Project(project) = controller
                .execute(controller.prepare(Command::RegisterProject {
                    name: "Terminal lifecycle".into(),
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
            let Output::Terminal(info) = controller
                .execute(
                    controller.prepare(Command::CreateTerminal(terminal::Launch {
                        worktree,
                        access: sailry_protocol::process::Access::Workspace,
                        viewport: viewport.clone(),
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
                delay_close,
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
                while !screens
                    .borrow()
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.info.status == terminal::Status::Running)
                {
                    screens.changed().await.unwrap();
                }
            })
            .await
            .unwrap();
            let revision = screens.borrow().snapshot.as_ref().unwrap().info.revision;
            assert!(connection.enqueue(Command::InputTerminal {
                terminal: info.id,
                revision: if close || release {
                    revision
                } else {
                    revision + 100
                },
                input: terminal::Input::Text {
                    text: "ignored".into()
                }
            }));
            tokio::time::timeout(Duration::from_secs(10), held.entered.cancelled())
                .await
                .unwrap();
            assert!(connection.enqueue(Command::ResizeTerminal {
                terminal: info.id,
                revision,
                viewport: viewport.clone()
            }));
            assert!(connection.enqueue(Command::SetTerminalAppearance {
                terminal: info.id,
                revision,
                appearance: appearance.clone()
            }));
            if close {
                controller
                    .execute(controller.prepare(Command::CloseTerminal {
                        worktree,
                        terminal: info.id,
                    }))
                    .await
                    .unwrap();
            }
            if release {
                connection.release();
            }
            held.release.cancel();
            let result = tokio::time::timeout(Duration::from_secs(10), results.recv())
                .await
                .unwrap();
            if close || release {
                assert!(
                    result.is_none(),
                    "released terminal commands must not report input failures"
                );
                assert!(connection.stopped());
                assert_eq!(connection.closed(), close);
                assert!(!connection.enqueue(Command::ResizeTerminal {
                    terminal: info.id,
                    revision,
                    viewport: viewport.clone()
                }));
            } else {
                assert_eq!(
                    result.unwrap().unwrap_err().code,
                    ErrorCode::RevisionConflict
                );
                assert!(
                    !connection.closed(),
                    "an active terminal conflict must remain visible"
                );
            }
            if !close {
                if release {
                    let Output::TerminalSnapshot(snapshot) = controller
                        .execute(controller.prepare(Command::InspectTerminal { terminal: info.id }))
                        .await
                        .unwrap()
                    else {
                        panic!("terminal snapshot expected")
                    };
                    assert_eq!(snapshot.info.status, terminal::Status::Running);
                    let (restored, mut recovered, _) =
                        Connection::start(&binding, appearance.clone());
                    tokio::time::timeout(Duration::from_secs(10), async {
                        while recovered.borrow().snapshot.is_none() {
                            recovered.changed().await.unwrap();
                        }
                    })
                    .await
                    .unwrap();
                    let recovered = recovered.borrow();
                    let recovered = recovered.snapshot.as_ref().unwrap();
                    assert_eq!(recovered.info.id, snapshot.info.id);
                    assert_eq!(recovered.info.status, terminal::Status::Running);
                    assert!(recovered.sequence >= snapshot.sequence);
                    drop(restored);
                }
                controller
                    .execute(controller.prepare(Command::CloseTerminal {
                        worktree,
                        terminal: info.id,
                    }))
                    .await
                    .unwrap();
            }
            assert_eq!(
                held.commands.lock().unwrap().len(),
                1,
                "queued resize and appearance commands must be dropped"
            );
            drop(connection);
        }
    }
    local.shutdown().await.unwrap();
    remote.shutdown().await.unwrap();
}
