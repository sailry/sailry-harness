use super::*;
use sailry_protocol::process::{Capture, Outcome};

#[tokio::test]
async fn executes_quoted_cmd_scripts() {
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
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let client = Client::new(controller.handle().remote(address));
    let release = sailry_link::CancellationToken::new();
    let server = Server::markdown_held("Command fixture finished".into(), release.clone()).await;
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
                                wait_ms: 0,
                                until: None,
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

fn captured(capture: &Capture, expected: &str) {
    assert_eq!(capture.text.trim(), expected);
    assert!(!capture.invalid_utf8);
    assert!(!capture.truncated);
}

mod decoding {
    use super::*;
    use sailry_protocol::process::{OutputStream, Snapshot, Status as ProcessStatus};
    use std::path::Path;
    use windows_sys::Win32::{
        Globalization::{GetOEMCP, MB_ERR_INVALID_CHARS, MultiByteToWideChar},
        System::{
            Console::GetConsoleOutputCP,
            Threading::{CREATE_NO_WINDOW, DETACHED_PROCESS},
        },
    };

    #[tokio::test]
    async fn preserves_system_output() {
        if std::env::var_os("SAILRY_CODEPAGE_FIXTURE").is_none() {
            // Isolate this fixture from other tests that change their inherited console code page.
            let output = tokio::time::timeout(
                Duration::from_secs(120),
                tokio::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "process_windows::decoding::preserves_system_output",
                        "--nocapture",
                    ])
                    .env("SAILRY_CODEPAGE_FIXTURE", "1")
                    .creation_flags(DETACHED_PROCESS)
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .expect("isolated code page fixture deadline")
            .unwrap();
            assert!(
                output.status.success(),
                "isolated code page fixture failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
            return;
        }
        // SAFETY: these queries have no pointers and do not change console state.
        let (console, code_page) = unsafe { (GetConsoleOutputCP(), GetOEMCP()) };
        assert_eq!(console, 0, "fixture must not inherit the runner console");
        assert_ne!(code_page, 0);
        let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("cmd.exe");
        let release = sailry_link::CancellationToken::new();
        let server =
            Server::markdown_held("Command fixture finished".into(), release.clone()).await;
        let fixture = process::Fixture::packages(&server, &[]).await;
        let raw = reference(&shell, &fixture.root, false).await;
        let wide = reference(&shell, &fixture.root, true).await;
        let wide = wide.strip_prefix(&[0xff, 0xfe]).unwrap_or(&wide);
        assert_eq!(wide.len() % 2, 0);
        let units: Vec<_> = wide
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let expected = String::from_utf16(&units).unwrap();
        assert!(!expected.is_empty());
        assert_eq!(decode(&raw, code_page), expected);
        let mut config = fixture.session.config.clone();
        config.permission = Permission::Full;
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
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
        for background in [false, true] {
            let request = fixture.client.prepare(Command::RunCommand {
                turn: turn.id,
                command: format!("\"{}\" /d /c ver", shell.display()),
                access: sailry_protocol::process::Access::Unrestricted,
                description: None,
                cwd: String::new(),
                timeout_ms: 10_000,
                pty: false,
                background,
                follow_up: false,
                attachments: Vec::new(),
            });
            let id = request.id;
            let response = fixture.client.execute(request).await.unwrap();
            let (outcome, stdout, stderr) = match response {
                Output::CommandResult(result) if !background => {
                    (result.outcome, result.stdout, result.stderr)
                }
                Output::CommandStarted(info) if background => {
                    assert_eq!(info.id, id);
                    assert!(!info.pty);
                    let snapshot = completed(&fixture.client, session.id, id).await;
                    let ProcessStatus::Finished(outcome) = snapshot.info.status else {
                        panic!("finished command expected")
                    };
                    (outcome, snapshot.stdout, snapshot.stderr)
                }
                other => panic!("command output expected: {other:?}"),
            };
            assert_eq!(outcome, Outcome::Exited(0), "background={background}");
            assert_eq!(stdout.text, expected);
            assert!(!stdout.invalid_utf8 && !stdout.truncated);
            assert_eq!(stderr, Capture::default());
            pages(&fixture, turn.id, id, &raw, &expected, code_page).await;
        }
        release.cancel();
        let page = finished(&fixture.client, session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
        // SAFETY: these queries have no pointers and do not change console state.
        assert_eq!(
            unsafe { (GetConsoleOutputCP(), GetOEMCP()) },
            (console, code_page)
        );
    }

    async fn pages(
        fixture: &process::Fixture,
        turn: TurnId,
        id: RequestId,
        raw: &[u8],
        expected: &str,
        code_page: u32,
    ) {
        for (stream, bytes, text) in [
            (OutputStream::Stdout, raw, expected),
            (OutputStream::Stderr, [].as_slice(), ""),
        ] {
            for limit in [1, 7] {
                let mut offset = 0;
                let mut actual = String::new();
                loop {
                    let Output::CommandOutputPage(page) = fixture
                        .client
                        .execute(fixture.client.prepare(Command::ReadCommandPage {
                            session: fixture.session.id,
                            id,
                            stream,
                            offset,
                            limit,
                            wait_ms: 0,
                            until: None,
                        }))
                        .await
                        .unwrap()
                    else {
                        panic!("command output page expected")
                    };
                    assert_eq!(page.info.id, id);
                    assert_eq!(page.info.session, fixture.session.id);
                    assert_eq!(page.info.turn, turn);
                    assert_eq!(
                        page.info.status,
                        ProcessStatus::Finished(Outcome::Exited(0))
                    );
                    assert!(!page.info.pty);
                    assert_eq!(page.stream, stream);
                    assert_eq!(page.offset, offset);
                    assert_eq!(page.total_bytes, bytes.len() as u64);
                    assert!(!page.invalid_utf8 && !page.truncated);
                    let end = page.next_offset.unwrap_or(bytes.len());
                    assert!(end <= bytes.len());
                    assert!(bytes.is_empty() || end > offset);
                    if bytes.is_empty() {
                        assert!(page.next_offset.is_none());
                    }
                    assert!(end - offset <= limit.max(4));
                    assert_eq!(page.text, decode(&bytes[offset..end], code_page));
                    actual.push_str(&page.text);
                    if limit == 1 && end - offset > 1 {
                        let error = fixture
                            .client
                            .execute(fixture.client.prepare(Command::ReadCommandPage {
                                session: fixture.session.id,
                                id,
                                stream,
                                offset: offset + 1,
                                limit,
                                wait_ms: 0,
                                until: None,
                            }))
                            .await
                            .unwrap_err();
                        assert_eq!(error.code, ErrorCode::InvalidRequest);
                    }
                    let Some(next) = page.next_offset else { break };
                    offset = next;
                }
                assert_eq!(actual, text);
            }
        }
    }

    async fn reference(shell: &Path, root: &Path, unicode: bool) -> Vec<u8> {
        let mut command = tokio::process::Command::new(shell);
        command.arg("/d");
        if unicode {
            command.arg("/u");
        }
        let output = tokio::time::timeout(
            Duration::from_secs(10),
            command
                .args(["/c", "ver"])
                .current_dir(root)
                .creation_flags(CREATE_NO_WINDOW)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("system command reference deadline")
        .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        output.stdout
    }

    async fn completed(client: &Client, session: SessionId, id: RequestId) -> Snapshot {
        tokio::time::timeout(COMPLETION_BUDGET, async {
            loop {
                let Output::CommandOutput(snapshot) = client
                    .execute(client.prepare(Command::ReadCommand {
                        session,
                        id,
                        wait_ms: 0,
                        until: None,
                    }))
                    .await
                    .unwrap()
                else {
                    panic!("command output expected")
                };
                assert_eq!(snapshot.info.id, id);
                match &snapshot.info.status {
                    ProcessStatus::Finished(_) => return snapshot,
                    ProcessStatus::Failed(error) => panic!("background command failed: {error}"),
                    ProcessStatus::Running | ProcessStatus::Stopping => {
                        tokio::task::yield_now().await
                    }
                }
            }
        })
        .await
        .expect("background command deadline")
    }

    fn decode(bytes: &[u8], code_page: u32) -> String {
        if bytes.is_empty() {
            return String::new();
        }
        let count = i32::try_from(bytes.len()).unwrap();
        // SAFETY: this call measures the UTF-16 capacity for the bounded input slice.
        let length = unsafe {
            MultiByteToWideChar(
                code_page,
                MB_ERR_INVALID_CHARS,
                bytes.as_ptr(),
                count,
                std::ptr::null_mut(),
                0,
            )
        };
        assert!(
            length > 0,
            "reference bytes must decode in the actual code page"
        );
        let mut wide = vec![0; length as usize];
        // SAFETY: both slices remain live and the measured output capacity is exact.
        let written = unsafe {
            MultiByteToWideChar(
                code_page,
                MB_ERR_INVALID_CHARS,
                bytes.as_ptr(),
                count,
                wide.as_mut_ptr(),
                length,
            )
        };
        assert_eq!(written, length);
        String::from_utf16(&wide).unwrap()
    }
}
