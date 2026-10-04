use super::*;

#[tokio::test]
async fn redraws_prompt() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join(".zshrc"),
        r#"setopt promptsubst
fixture_prompt() { sleep 0.03; print -rn '%K{magenta}resize-fixture%k %F{green}project%f > '; }
PROMPT='$(fixture_prompt)'
RPROMPT=''
"#,
    )
    .unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let local = Client::new(node.local());
    local
        .execute(local.prepare(Command::RegisterProject {
            name: "Resize fixture".into(),
            path: directory.path().to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    local
        .execute(local.prepare(Command::SaveTerminalSettings(Settings {
            shell: "/bin/zsh".into(),
            environment: [("HOME".into(), directory.path().to_str().unwrap().into())].into(),
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
    let tree = snapshot.worktrees[0].id;
    for transport in [node.local(), controller.handle().remote(address)] {
        let client = Client::new(transport);
        let Output::Terminal(info) = client
            .execute(client.prepare(Command::CreateTerminal(launch(tree))))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        until(&mut *stream, &mut projection, "resize-fixture").await;
        command(&client, &info, "printf output-without-newline").await;
        for columns in (80..220).rev().chain(80..220) {
            client
                .execute(client.prepare(Command::ResizeTerminal {
                    terminal: info.id,
                    revision: info.revision,
                    viewport: Viewport {
                        columns,
                        rows: columns / 3,
                        pixel_width: u32::from(columns) * 8,
                        pixel_height: u32::from(columns / 3) * 16,
                    },
                }))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        // A subscription opened after the last shell redraw observes the canonical screen.
        tokio::time::sleep(Duration::from_millis(150)).await;
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        projection.apply(1, stream.next().await.unwrap()).unwrap();
        let screen = &projection.snapshot().unwrap().screen;
        let text = content(screen);
        assert!(text.ends_with("resize-fixture project > "), "{text:?}");
        for span in screen
            .scrollback
            .iter()
            .chain(&screen.rows)
            .flat_map(|line| &line.spans)
        {
            if span.style.background.is_some() && !span.text.trim().is_empty() {
                assert_eq!(span.text.trim(), "resize-fixture", "{text:?}");
            }
        }
        client
            .execute(client.prepare(Command::CloseTerminal {
                worktree: tree,
                terminal: info.id,
            }))
            .await
            .unwrap();
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
