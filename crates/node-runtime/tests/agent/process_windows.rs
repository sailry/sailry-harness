use super::*;
use sailry_protocol::process::{Capture, Outcome};

#[tokio::test]
async fn executes_quoted_cmd_scripts() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        let nested = root.join("项目 with spaces");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("quoted script.cmd"),
            "@echo off\r\nchcp 65001>nul\r\nif defined TERMINAL_FIXTURE exit /b 99\r\necho \"quoted & 中文 🙂\"\r\necho error-marker 1>&2\r\nexit /b 7\r\n",
        )
        .unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let release = sailry_link::CancellationToken::new();
        let server =
            Server::markdown_held("Command fixture finished".into(), release.clone()).await;
        let mut session = approvals::prepare_packages(&client, &server, &root, &[]).await;
        let mut config = session.config.clone();
        config.permission = Permission::Full;
        let Output::Session(updated) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        session = updated;
        let Output::TerminalSettings(mut settings) = client
            .execute(client.prepare(Command::ReadTerminalSettings))
            .await
            .unwrap()
        else {
            panic!("terminal settings expected")
        };
        settings.shell = r"C:\Interactive Shell\pwsh.exe".into();
        settings
            .environment
            .insert("TERMINAL_FIXTURE".into(), "interactive-only".into());
        client
            .execute(client.prepare(Command::SaveTerminalSettings(settings)))
            .await
            .unwrap();
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Keep the command fixture active".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        server.wait_count(1).await;
        let request = server.requests.lock().unwrap()[0].clone();
        let shell = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
        assert!(turn_context(&request).contains(&format!("Command shell: {shell:?}")));
        for (background, pty) in [(false, false), (true, false), (true, true)] {
            let request = client.prepare(Command::RunCommand {
                turn: turn.id,
                command: "\"quoted script.cmd\"".into(),
                access: sailry_protocol::process::Access::Unrestricted,
                description: None,
                cwd: "项目 with spaces".into(),
                timeout_ms: 10_000,
                pty,
                background,
                follow_up: false,
                attachments: Vec::new(),
            });
            let response = client.execute(request.clone()).await.unwrap();
            let (outcome, stdout, stderr) = match response.clone() {
                Output::CommandResult(result) if !background => {
                    (result.outcome, result.stdout, result.stderr)
                }
                Output::CommandStarted(info) if background => {
                    let snapshot = tokio::time::timeout(Duration::from_secs(10), async {
                        loop {
                            let Output::CommandOutput(snapshot) = client
                                .execute(client.prepare(Command::ReadCommand {
                                    session: session.id,
                                    id: info.id,
                                }))
                                .await
                                .unwrap()
                            else {
                                panic!("command output expected")
                            };
                            if matches!(
                                snapshot.info.status,
                                sailry_protocol::process::Status::Finished(_)
                            ) {
                                break snapshot;
                            }
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .expect("background command deadline");
                    let sailry_protocol::process::Status::Finished(outcome) = snapshot.info.status
                    else {
                        unreachable!()
                    };
                    (outcome, snapshot.stdout, snapshot.stderr)
                }
                other => panic!("command output expected: {other:?}"),
            };
            assert_eq!(outcome, Outcome::Exited(7));
            if pty {
                assert!(stdout.text.contains("\"quoted & 中文 🙂\""));
                assert!(stdout.text.contains("error-marker"));
                assert!(!stdout.invalid_utf8);
                assert!(!stdout.truncated);
                assert_eq!(stderr, Capture::default());
            } else {
                captured(&stdout, "\"quoted & 中文 🙂\"");
                captured(&stderr, "error-marker");
            }
            assert_eq!(client.execute(request).await.unwrap(), response);
        }
        release.cancel();
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

fn captured(capture: &Capture, expected: &str) {
    assert_eq!(capture.text.trim(), expected);
    assert!(!capture.invalid_utf8);
    assert!(!capture.truncated);
}
