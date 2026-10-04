use sailry_client::{Client, terminal::Projection};
use sailry_link::{Link, NetworkScope, Subscription, Transport};
use sailry_node_runtime::Node;
use sailry_protocol::{terminal::*, *};
use std::{sync::Arc, time::Duration};
#[path = "terminal/activity.rs"]
mod activity;
#[path = "terminal/reopen.rs"]
mod reopen;
#[path = "terminal/resize.rs"]
#[cfg(target_os = "macos")]
mod resize;
#[path = "terminal/settings.rs"]
mod settings;
#[path = "terminal/synchronized_output.rs"]
#[cfg(unix)]
mod synchronized_output;
#[path = "terminal/tools.rs"]
#[cfg(unix)]
mod tools;

fn launch(worktree: WorktreeId) -> Launch {
    Launch {
        worktree,
        viewport: Viewport {
            columns: 80,
            rows: 24,
            pixel_width: 640,
            pixel_height: 384,
        },
        appearance: Appearance {
            foreground: Rgb {
                red: 240,
                green: 240,
                blue: 240,
            },
            background: Rgb {
                red: 20,
                green: 20,
                blue: 20,
            },
            palette: [Rgb {
                red: 128,
                green: 128,
                blue: 128,
            }; 16],
            color_scheme: ColorScheme::Dark,
        },
    }
}

async fn command(client: &Client, info: &Info, text: &str) {
    client
        .execute(client.prepare(Command::InputTerminal {
            terminal: info.id,
            revision: info.revision,
            input: Input::Paste { text: text.into() },
        }))
        .await
        .unwrap();
    client
        .execute(client.prepare(Command::InputTerminal {
            terminal: info.id,
            revision: info.revision,
            input: Input::Key {
                event: KeyEvent {
                    key: Key::Enter,
                    action: Action::Press,
                    modifiers: Modifiers::default(),
                    utf8: None,
                    unshifted_codepoint: None,
                },
            },
        }))
        .await
        .unwrap();
}

fn content(screen: &Screen) -> String {
    screen
        .scrollback
        .iter()
        .chain(&screen.rows)
        .flat_map(|line| line.spans.iter().map(|span| span.text.as_str()))
        .collect()
}

async fn until(stream: &mut dyn Subscription, projection: &mut Projection, needle: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            projection.apply(1, stream.next().await.unwrap()).unwrap();
            if projection
                .snapshot()
                .is_some_and(|snapshot| content(&snapshot.screen).contains(needle))
            {
                break;
            }
        }
    })
    .await
    .expect("terminal output deadline");
}

#[tokio::test]
async fn shares_process_and_input_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let transports: [Arc<dyn Transport>; 2] = [node.local(), controller.handle().remote(address)];
    let local = Client::new(transports[0].clone());
    local
        .execute(local.prepare(Command::RegisterProject {
            name: "Terminal fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = local
        .execute(local.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let worktree = snapshot.worktrees[0].id;
    let count = || {
        rusqlite::Connection::open(node.profile().join("storage/node.sqlite3"))
            .unwrap()
            .query_row("SELECT count(*) FROM requests", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    for index in 0..2 {
        let client = Client::new(transports[index].clone());
        let other = Client::new(transports[1 - index].clone());
        let request = client.prepare(Command::CreateTerminal(launch(worktree)));
        let before = count();
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let output = admitted.completion.await.unwrap().unwrap();
        assert_eq!(client.execute(request).await.unwrap(), output);
        let Output::Terminal(mut info) = output else {
            panic!("terminal expected")
        };
        let mut stream = other.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        projection.apply(1, stream.next().await.unwrap()).unwrap();
        command(&client, &info, "printf '%s%s\\n' 'pty-' 'ready'").await;
        until(&mut *stream, &mut projection, "pty-ready").await;
        assert_eq!(
            count(),
            before + 1,
            "input and screen updates must not enter the durable ledger"
        );
        let denied = other
            .execute(other.prepare(Command::InputTerminal {
                terminal: info.id,
                revision: info.revision,
                input: Input::Text {
                    text: "blocked".into(),
                },
            }))
            .await
            .unwrap_err();
        assert_eq!(denied.code, ErrorCode::PermissionDenied);
        let Output::Terminal(claimed) = other
            .execute(other.prepare(Command::ClaimTerminal {
                terminal: info.id,
                expected_revision: info.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("claimed terminal expected")
        };
        assert_eq!(claimed.revision, info.revision + 1);
        assert_eq!(
            client
                .execute(client.prepare(Command::InputTerminal {
                    terminal: info.id,
                    revision: info.revision,
                    input: Input::Text {
                        text: "stale".into()
                    }
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        info = claimed;
        let Output::TerminalSnapshot(resized) = other
            .execute(other.prepare(Command::ResizeTerminal {
                terminal: info.id,
                revision: info.revision,
                viewport: Viewport {
                    columns: 100,
                    rows: 30,
                    pixel_width: 800,
                    pixel_height: 480,
                },
            }))
            .await
            .unwrap()
        else {
            panic!("screen expected")
        };
        assert_eq!(resized.screen.columns, 100);
        assert_eq!(resized.screen.rows.len(), 30);
        command(&other, &info, "stty size").await;
        until(&mut *stream, &mut projection, "30 100").await;
        drop(stream);
        controller.handle().disconnect(node.id()).await;
        command(&other, &info, "printf '%s%s\\n' 'still-' 'running'").await;
        let mut recovered = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        until(&mut *recovered, &mut projection, "still-running").await;
        #[cfg(unix)]
        {
            command(&other, &info, "vim -Nu NONE -i NONE -n --cmd 'set mouse=a'").await;
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    projection
                        .apply(1, recovered.next().await.unwrap())
                        .unwrap();
                    if projection.snapshot().is_some_and(|snapshot| {
                        snapshot.screen.alternate && snapshot.screen.mouse_tracking
                    }) {
                        break;
                    }
                }
            })
            .await
            .expect("foreground TUI deadline");
        }
        let closing = other.prepare(Command::CloseTerminal {
            worktree,
            terminal: info.id,
        });
        let closed = tokio::time::timeout(Duration::from_secs(5), other.execute(closing.clone()))
            .await
            .expect("closing a foreground TUI must release the PTY before waiting")
            .unwrap();
        assert!(matches!(&closed, Output::Terminal(info) if info.status == Status::Closed));
        assert_eq!(other.execute(closing).await.unwrap(), closed);
        assert_eq!(count(), before + 2);
    }
    drop(transports);
    controller.close().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), node.shutdown())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn preserves_stopped_processes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Terminal fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let worktree = snapshot.worktrees[0].id;
    let request = client.prepare(Command::CreateTerminal(launch(worktree)));
    let original = client.execute(request.clone()).await.unwrap();
    let Output::Terminal(info) = &original else {
        panic!("terminal expected")
    };
    command(&client, info, "sleep 30").await;
    let mut stream = client.subscribe_terminal(info.id).await.unwrap();
    stream.next().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), node.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .is_err()
    );
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let Output::Terminals(infos) = client
        .execute(client.prepare(Command::ListTerminals { worktree }))
        .await
        .unwrap()
    else {
        panic!("terminals expected")
    };
    assert_eq!(infos.len(), 1);
    assert_ne!(infos[0].status, Status::Running);
    assert_eq!(client.execute(request).await.unwrap(), original);
    assert!(client.subscribe_terminal(info.id).await.is_err());
    node.shutdown().await.unwrap();
}
