use super::*;
use sailry_link::{Admission, CancellationToken, Pending};
use sailry_protocol::terminal::{Frame, Snapshot};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::watch;

struct Malformed {
    inner: Arc<dyn Transport>,
    subscriptions: AtomicUsize,
    commands: AtomicUsize,
    reopening: CancellationToken,
    release: CancellationToken,
}

impl Transport for Malformed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn open(&self, id: StreamId) -> Pending<'_, Result<sailry_link::Stream, Fault>> {
        self.inner.open(id)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        self.commands.fetch_add(1, Ordering::SeqCst);
        self.inner.dispatch(request)
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            let attempt = self.subscriptions.fetch_add(1, Ordering::SeqCst);
            if attempt > 0 {
                self.reopening.cancel();
                self.release.cancelled().await;
            }
            let inner = self.inner.subscribe(topic).await?;
            if attempt == 0 {
                Ok(Box::new(Frames {
                    inner,
                    initial: None,
                }) as Box<dyn Subscription>)
            } else {
                Ok(inner)
            }
        })
    }
}

struct Frames {
    inner: Box<dyn Subscription>,
    initial: Option<Snapshot>,
}

impl Subscription for Frames {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if let Some(snapshot) = self.initial.take() {
                let dropped_scrollback_rows = snapshot.screen.scrollback.len() as u32;
                let mut screen = snapshot.screen;
                screen.rows[0].spans = vec![Span {
                    column: screen.columns - 1,
                    columns: 2,
                    text: "界".into(),
                    style: Default::default(),
                    hyperlink: None,
                }];
                return Ok(Update::TerminalFrame(Frame {
                    node: snapshot.node,
                    info: snapshot.info,
                    sequence: snapshot.sequence + 1,
                    screen: ScreenUpdate::Patch {
                        patch: Patch {
                            dropped_scrollback_rows,
                            appended_scrollback: screen.scrollback,
                            columns: screen.columns,
                            rows: screen.rows,
                            cursor: screen.cursor,
                            foreground: screen.foreground,
                            background: screen.background,
                            cursor_color: screen.cursor_color,
                            bracketed_paste: screen.bracketed_paste,
                            mouse_tracking: screen.mouse_tracking,
                            alternate: screen.alternate,
                            features: screen.features,
                            graphics: Some(screen.graphics),
                        },
                    },
                }));
            }
            let update = self.inner.next().await?;
            if let Update::TerminalSnapshot(snapshot) = &update {
                self.initial = Some(snapshot.clone());
            }
            Ok(update)
        })
    }
}

async fn receive(
    updates: &mut watch::Receiver<sailry_client::terminal::View>,
    predicate: impl Fn(&sailry_client::terminal::View) -> bool,
) -> sailry_client::terminal::View {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let view = updates.borrow_and_update().clone();
            if predicate(&view) {
                return view;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .expect("terminal recovery deadline")
}

#[tokio::test]
async fn restores_malformed_screen_without_replaying_commands() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let home = directory.path().join("shell-home");
    std::fs::create_dir(&root).unwrap();
    let node = isolated(&directory.path().join("node"), &home).await;
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let local = Client::new(node.local());
    let Output::Project(project) = local
        .execute(local.prepare(Command::RegisterProject {
            name: "Terminal recovery".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    local
        .execute(local.prepare(Command::SaveTerminalSettings(Settings {
            shell: "/bin/sh".into(),
            environment: [("HOME".into(), home.to_str().unwrap().into())].into(),
            ..Default::default()
        })))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = local
        .execute(local.prepare(Command::Snapshot))
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
    for transport in [node.local(), controller.handle().remote(address)] {
        let client = Client::new(transport.clone());
        let Output::Terminal(info) = client
            .execute(client.prepare(Command::CreateTerminal(launch(worktree))))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        // ASCII input keeps line editing independent of the fixture's C locale.
        command(
            &client,
            &info,
            "printf '\\344\\270\\255\\346\\226\\207\\350\\276\\271\\347\\225\\214\\n'",
        )
        .await;
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        until(&mut *stream, &mut projection, "中文边界").await;
        drop(stream);

        let malformed = Arc::new(Malformed {
            inner: transport,
            subscriptions: AtomicUsize::new(0),
            commands: AtomicUsize::new(0),
            reopening: CancellationToken::new(),
            release: CancellationToken::new(),
        });
        let observing = Client::new(malformed.clone());
        let (sender, mut views) = watch::channel(Default::default());
        let stop = CancellationToken::new();
        let observing_stop = stop.clone();
        let terminal = info.id;
        let task = tokio::spawn(async move {
            observing
                .watch_terminal(terminal, sender, observing_stop)
                .await
        });
        let recovering = receive(&mut views, |view| view.recovering).await;
        assert!(!recovering.connected);
        assert!(recovering.error.is_none());
        let retained = recovering.snapshot.unwrap();
        assert_eq!(retained.info.id, info.id);
        assert!(content(&retained.screen).contains("中文边界"));
        retained.screen.validate().unwrap();
        tokio::time::timeout(Duration::from_secs(10), malformed.reopening.cancelled())
            .await
            .unwrap();
        assert!(views.borrow().recovering);
        malformed.release.cancel();
        let recovered = receive(&mut views, |view| view.connected).await;
        assert!(!recovered.recovering);
        assert!(recovered.error.is_none());
        assert!(recovered.snapshot.unwrap().sequence >= retained.sequence);
        assert_eq!(malformed.subscriptions.load(Ordering::SeqCst), 2);
        assert_eq!(malformed.commands.load(Ordering::SeqCst), 0);
        stop.cancel();
        task.await.unwrap().unwrap();

        for columns in (1..=24).rev().chain(2..=24) {
            let Output::TerminalSnapshot(resized) = client
                .execute(client.prepare(Command::ResizeTerminal {
                    terminal: info.id,
                    revision: info.revision,
                    viewport: Viewport {
                        columns,
                        rows: 4,
                        pixel_width: u32::from(columns) * 8,
                        pixel_height: 64,
                    },
                }))
                .await
                .unwrap()
            else {
                panic!("terminal snapshot expected")
            };
            assert_eq!(resized.screen.columns, columns);
            resized.screen.validate().unwrap();
        }
        client
            .execute(client.prepare(Command::CloseTerminal {
                worktree,
                terminal: info.id,
            }))
            .await
            .unwrap();
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
