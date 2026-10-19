use super::{
    fixture::{Fixture, launch},
    *,
};
use std::{sync::Arc, time::Duration};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preserves_access_while_another_terminal_starts() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    for (remote, stopping) in [(false, false), (true, false), (false, true), (true, true)] {
        let fixture = Fixture::new(remote, "/bin/sh").await;
        let Output::Terminal(first) = fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::CreateTerminal(launch(fixture.worktrees[0]))),
            )
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let gate = std::sync::Mutex::new(Some((entered, wait)));
        fixture.node.ingress.terminals.on_start(Arc::new(move || {
            let gate = gate.lock().unwrap().take();
            if let Some((entered, wait)) = gate {
                let _ = entered.send(());
                let _ = wait.recv();
            }
        }));
        let opening = {
            let client = fixture.client.clone();
            let launch = launch(fixture.worktrees[1]);
            tokio::spawn(async move {
                client
                    .execute(client.prepare(Command::CreateTerminal(launch)))
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .expect("the reserved start must reach the barrier")
            .unwrap();
        let mut phase = "inspect";
        let accessed = tokio::time::timeout(Duration::from_secs(5), async {
            assert!(matches!(
                fixture
                    .client
                    .execute(
                        fixture
                            .client
                            .prepare(Command::InspectTerminal { terminal: first.id })
                    )
                    .await
                    .unwrap(),
                Output::TerminalSnapshot(_)
            ));
            phase = "input";
            fixture
                .client
                .execute(fixture.client.prepare(Command::InputTerminal {
                    terminal: first.id,
                    revision: first.revision,
                    input: terminal::Input::Paste {
                        text: "printf 'still active'".into(),
                    },
                }))
                .await
                .unwrap();
            phase = "close";
            fixture
                .client
                .execute(fixture.client.prepare(Command::CloseTerminal {
                    worktree: fixture.worktrees[0],
                    terminal: first.id,
                }))
                .await
                .unwrap();
        })
        .await;
        if stopping {
            fixture.node.ingress.terminals.stop();
        }
        let _ = release.send(());
        accessed.unwrap_or_else(|_| {
            panic!("terminal {phase} must not wait for another terminal's initialization")
        });
        let result = opening.await.unwrap();
        if stopping {
            assert!(matches!(result, Err(error) if error.code == ErrorCode::Unavailable));
        } else {
            assert!(matches!(result.unwrap(), Output::Terminal(_)));
        }
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shares_same_id_after_initialization() {
    let fixture = Fixture::new(false, "/bin/sh").await;
    let previous = Info {
        id: TerminalId::new(),
        worktree: Some(fixture.worktrees[0]),
        ssh: None,
        tool: None,
        access: Some(sailry_protocol::process::Access::Unrestricted),
        status: Status::Stopped,
        activity: None,
        title: None,
        directory: None,
        owner: None,
        revision: 4,
    };
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let gate = std::sync::Mutex::new(Some((entered, wait)));
    fixture.node.ingress.terminals.on_start(Arc::new(move || {
        if let Some((entered, wait)) = gate.lock().unwrap().take() {
            let _ = entered.send(());
            let _ = wait.recv();
        }
    }));
    let opening = || {
        let terminals = fixture.node.ingress.terminals.clone();
        let environment = fixture.node.ingress.environment.clone();
        let root = fixture.directory.path().join("first");
        let home = fixture.directory.path().join("home");
        let previous = previous.clone();
        let launch = launch(fixture.worktrees[0]);
        tokio::task::spawn_blocking(move || {
            terminals.open(
                NodeId([73; 32]),
                &previous,
                &launch.viewport,
                &launch.appearance,
                crate::terminal::Source::Local {
                    root,
                    profile: None,
                    access: sailry_protocol::process::Access::Unrestricted,
                    settings: terminal::Settings {
                        shell: "/bin/sh".into(),
                        environment: [("HOME".into(), home.to_string_lossy().into_owned())].into(),
                        ..Default::default()
                    },
                    tool: None,
                    environment,
                },
            )
        })
    };
    let first = opening();
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .expect("the reserved start must reach the barrier")
        .unwrap();
    let second = opening();
    let _ = release.send(());
    let first = first.await.unwrap().unwrap();
    let second = second.await.unwrap().unwrap();
    assert_eq!(first.id, previous.id);
    assert_eq!(second.id, previous.id);
    assert_eq!(first.revision, 5);
    assert_eq!(second.revision, 5);
    assert_eq!(
        fixture
            .node
            .ingress
            .terminals
            .existing(previous.id)
            .unwrap()
            .unwrap()
            .revision,
        5
    );
    fixture.close().await;
}
