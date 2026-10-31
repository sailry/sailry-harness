use super::*;

const PROMPT: &str = "➜  sailry-public git:(main) ";

fn content(screen: &terminal::Screen) -> String {
    screen
        .scrollback
        .iter()
        .chain(&screen.rows)
        .flat_map(|line| line.spans.iter().map(|span| span.text.as_str()))
        .collect()
}

async fn settled(client: &Client, id: TerminalId, prompt: &str) -> terminal::Screen {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut previous = None;
        loop {
            let Output::TerminalSnapshot(snapshot) = client
                .execute(client.prepare(Command::InspectTerminal { terminal: id }))
                .await
                .unwrap()
            else {
                panic!("terminal snapshot expected")
            };
            let screen = snapshot.screen;
            if screen.columns == 80
                && content(&screen).ends_with(prompt)
                && previous.as_ref() == Some(&screen)
            {
                return screen;
            }
            previous = Some(screen);
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    })
    .await
    .expect("terminal prompt settlement deadline")
}

async fn signal(path: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.is_file() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("async prompt handshake deadline");
}

async fn resized(client: &Client, id: TerminalId, columns: u16) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let Output::TerminalSnapshot(snapshot) = client
                .execute(client.prepare(Command::InspectTerminal { terminal: id }))
                .await
                .unwrap()
            else {
                panic!("terminal snapshot expected")
            };
            if snapshot.screen.columns == columns {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("terminal resize admission deadline");
}

#[tokio::test]
async fn survives_geometry_bursts() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("shell-home");
    let root = directory.path().join("project");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        home.join(".zshrc"),
        r#"setopt promptsubst
autoload -Uz add-zsh-hook
fixture_segment=''
fixture_git() { print -rn -- "$fixture_segment"; }
PROMPT='%F{green}%1{➜%}  %F{cyan}sailry-public%f $(fixture_git)'
RPROMPT=''
fixture_complete() {
    local fd=$1
    IFS= read -r -u "$fd" fixture_segment
    : > "$HOME/resize-entered"
    while [[ ! -f "$HOME/resize-redraw" ]]; do sleep 0.005; done
    zle .reset-prompt
    zle -R
    : > "$HOME/resize-updated"
    exec {fd}<&-
    zle -F "$fd"
}
fixture_request() {
    local fd
    exec {fd}< <(
        while [[ ! -f "$HOME/resize-ready" ]]; do sleep 0.005; done
        sleep 0.03
        print -r -- '%F{blue}git:(%F{red}main%F{blue})%f '
    )
    zle -F "$fd" fixture_complete
    add-zsh-hook -d precmd fixture_request
}
add-zsh-hook precmd fixture_request
"#,
    )
    .unwrap();
    let node = Node::start_empty_with_environment(
        directory.path().join("node"),
        sailry_node_runtime::EnvironmentFixture {
            home: home.clone(),
            shell: "/usr/bin/false".into(),
            environment: [
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("LANG".into(), "en_US.UTF-8".into()),
            ]
            .into(),
            timeout: Duration::from_secs(5),
        },
    )
    .await
    .unwrap();
    let link = sailry_link::Link::controller(
        directory.path().join("controller"),
        sailry_link::NetworkScope::default(),
    )
    .await
    .unwrap();
    let address = link
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let local = Client::new(node.local());
    let Output::Project(project) = local
        .execute(local.prepare(Command::RegisterProject {
            name: "Terminal prompt".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    local
        .execute(
            local.prepare(Command::SaveTerminalSettings(terminal::Settings {
                shell: "/bin/zsh".into(),
                environment: [("HOME".into(), home.to_str().unwrap().into())].into(),
                ..Default::default()
            })),
        )
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
        .find(|tree| tree.project == Some(project.id) && tree.main)
        .unwrap()
        .id;
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
    for (lane, transport) in [node.local(), link.handle().remote(address)]
        .into_iter()
        .enumerate()
    {
        for name in [
            "resize-ready",
            "resize-entered",
            "resize-redraw",
            "resize-updated",
        ] {
            let path = home.join(name);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        let controller = Client::new(transport.clone());
        let Output::Terminal(info) = controller
            .execute(
                controller.prepare(Command::CreateTerminal(terminal::Launch {
                    worktree,
                    access: sailry_protocol::process::Access::Unrestricted,
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
            inner: transport,
            entered: CancellationToken::new(),
            release: CancellationToken::new(),
            screen: CancellationToken::new(),
            delay_close: false,
            reject_resize: AtomicBool::new(false),
            commands: Mutex::new(Vec::new()),
        });
        let binding = Binding {
            client: Arc::new(Client::new(held.clone())),
            caller: node.id(),
            id: info.id,
            runtime: tokio::runtime::Handle::current(),
            scope: None,
        };
        let (connection, mut screens, mut results) =
            Connection::start(&binding, appearance.clone());
        let completed = tokio::spawn(async move {
            while let Some(result) = results.recv().await {
                result.unwrap();
            }
        });
        let initial = settled(&controller, info.id, "➜  sailry-public ").await;
        assert_eq!(content(&initial).trim(), "➜  sailry-public");
        // Produce layout samples independently of command completion, as the
        // sidebar's 180 ms animation does at 15 ms frame intervals.
        let opening = [80, 77, 67, 61, 58, 55, 53, 52, 51, 50, 50, 50, 50];
        let closing = [50, 52, 62, 68, 71, 74, 76, 77, 78, 79, 79, 79, 80];
        let produced: Vec<_> = opening
            .into_iter()
            .chain(closing)
            .cycle()
            .take(78)
            .collect();
        for (sample, columns) in produced.iter().enumerate() {
            assert!(connection.enqueue(Command::ResizeTerminal {
                terminal: info.id,
                revision: info.revision,
                viewport: viewport(*columns),
            }));
            if sample == 2 {
                std::fs::write(home.join("resize-ready"), "").unwrap();
                signal(&home.join("resize-entered")).await;
            } else if sample == 3 {
                // Hold the async ZLE callback until a distinct geometry sample
                // has reached the PTY, then release reset-prompt before the
                // remaining animation frames. This forces the interleaving.
                resized(&controller, info.id, *columns).await;
                std::fs::write(home.join("resize-redraw"), "").unwrap();
                signal(&home.join("resize-updated")).await;
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        let screen = settled(&controller, info.id, PROMPT).await;
        let dispatched: Vec<_> = held
            .commands
            .lock()
            .unwrap()
            .iter()
            .filter_map(|command| match command {
                Command::ResizeTerminal { viewport, .. } => Some(viewport.columns),
                _ => None,
            })
            .collect();
        eprintln!(
            "terminal lane {lane} dispatched {}/{} geometry samples: {dispatched:?}",
            dispatched.len(),
            produced.len()
        );
        assert_eq!(dispatched.last(), Some(&80));
        let mut remaining = produced.iter();
        for columns in &dispatched {
            assert!(remaining.by_ref().any(|sample| sample == columns));
        }
        assert_eq!(
            content(&screen).trim(),
            PROMPT.trim(),
            "idle prompt must not accumulate redraws"
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            while screens
                .borrow()
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| snapshot.screen != screen)
            {
                screens.changed().await.unwrap();
            }
        })
        .await
        .expect("terminal projection settlement deadline");
        connection.release();
        completed.await.unwrap();
        controller
            .execute(controller.prepare(Command::CloseTerminal {
                worktree,
                terminal: info.id,
            }))
            .await
            .unwrap();
    }
    link.close().await.unwrap();
    node.shutdown().await.unwrap();
}
