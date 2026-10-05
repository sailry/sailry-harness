use super::{Duration, NodeId, Path, Session};
use sailry_client::{Client, conversation::Projection};
use sailry_node_runtime::Node;
use sailry_protocol::{
    Authentication, Command, Effort, Output, Permission, ProviderId, SessionConfig, WorkMode,
    conversation::{Model, ModelApi, Provider, Status},
    terminal::*,
};

pub(super) async fn prepare(
    root: &Path,
    browser: Option<String>,
    capture: bool,
) -> (Node, crate::agent_fixture::Server, [(NodeId, Session); 2]) {
    let local = Node::start(root.join("local")).await.unwrap();
    let remote = Node::start(root.join("remote")).await.unwrap();
    local
        .link()
        .pair(remote.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let server = if let Some(url) = &browser {
        if capture {
            crate::agent_fixture::Server::browser(url.clone()).await
        } else {
            crate::agent_fixture::Server::browser_interactions(url.clone()).await
        }
    } else {
        crate::agent_fixture::Server::workload().await
    };
    let mut sessions = Vec::new();
    for (name, node) in [("Local workload", &local), ("Remote workload", &remote)] {
        let project_root = root.join(name);
        std::fs::create_dir(&project_root).unwrap();
        std::fs::write(
            project_root.join("source.txt"),
            "Tool output fixture 中文 🙂\n".repeat(1000),
        )
        .unwrap();
        let client = Client::new(node.local());
        let Output::Project(project) = execute(
            &client,
            Command::RegisterProject {
                name: name.into(),
                path: project_root.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("project expected")
        };
        let provider = ProviderId::new();
        execute(
            &client,
            Command::PutProvider {
                expected_revision: 0,
                provider: Provider {
                    options: None,
                    oauth: None,
                    id: provider,
                    revision: 0,
                    name: "Isolated model".into(),
                    api: ModelApi::ChatCompletions,
                    authentication: Authentication::ApiKey,
                    endpoint: server.endpoint.clone(),
                    enabled: true,
                    models: vec![Model {
                        id: "fixture".into(),
                        context: 262144,
                        output: 4096,
                        vision: false,
                        tools: true,
                        reasoning: false,
                        web_search: false,
                        generates: vec![],
                        efforts: vec![],
                        custom_efforts: false,
                        default_effort: Effort::Default,
                    }],
                    default_model: "fixture".into(),
                    credential: None,
                },
            },
        )
        .await;
        let Output::Session(session) = execute(
            &client,
            Command::CreateSession {
                project: Some(project.id),
                worktree: None,
                config: Some(SessionConfig {
                    assistant: None,
                    resource: None,
                    provider,
                    model: "fixture".into(),
                    effort: Effort::Default,
                    mode: WorkMode::Code,
                    permission: Permission::Full,
                    credential: None,
                }),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        if browser.is_none() {
            turns(&client, &session, "Seed ", 24).await;
        }
        sessions.push((node.id(), session));
    }
    local.shutdown().await.unwrap();
    (remote, server, sessions.try_into().unwrap())
}

pub(super) async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

pub(super) async fn turns(client: &Client, session: &Session, prefix: &str, count: usize) {
    let mut subscription = client.subscribe_conversation(session.id).await.unwrap();
    let mut projection = Projection::new(client.target(), session.id, 1);
    projection
        .apply(1, subscription.next().await.unwrap())
        .unwrap();
    for index in 0..count {
        let Output::QueuedTurn(turn) = execute(
            client,
            Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: format!("{prefix}{index}: Read source.txt and explain the result").into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                projection
                    .apply(1, subscription.next().await.unwrap())
                    .unwrap();
                if let Some(run) = projection
                    .snapshot()
                    .unwrap()
                    .page
                    .runs
                    .iter()
                    .find(|run| run.turn == turn.id)
                    && !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
                {
                    assert_eq!(run.status, Status::Completed, "{:?}", run.error);
                    assert!(projection.snapshot().unwrap().page.entries.iter()
                        .filter(|entry| entry.turn == turn.id)
                        .flat_map(|entry| &entry.parts)
                        .any(|part| matches!(part, sailry_protocol::conversation::Part::ToolResult { name, result, .. }
                            if name == "read_file" && result.to_string().contains("Tool output fixture"))),
                        "completed turn must include the actual file tool result");
                    break;
                }
            }
        })
        .await
        .expect("conversation workload deadline");
    }
}

pub(super) async fn terminal(client: &Client, session: &Session) -> Info {
    let color = Rgb {
        red: 128,
        green: 128,
        blue: 128,
    };
    let Output::Terminal(info) = execute(
        client,
        Command::CreateTerminal(Launch {
            worktree: session.worktree,
            viewport: Viewport {
                columns: 100,
                rows: 30,
                pixel_width: 800,
                pixel_height: 480,
            },
            appearance: Appearance {
                foreground: color,
                background: color,
                palette: [color; 16],
                color_scheme: ColorScheme::Dark,
            },
        }),
    )
    .await
    else {
        panic!("terminal expected")
    };
    info
}

pub(super) async fn output(client: &Client, info: &Info) {
    let mut subscription = client.subscribe_terminal(info.id).await.unwrap();
    let mut screen = sailry_client::terminal::Projection::new(client.target(), info.id, 1);
    screen.apply(1, subscription.next().await.unwrap()).unwrap();
    let Output::TerminalSnapshot(current) =
        execute(client, Command::InspectTerminal { terminal: info.id }).await
    else {
        panic!("terminal expected")
    };
    let Output::Terminal(info) = execute(
        client,
        Command::ClaimTerminal {
            terminal: info.id,
            expected_revision: current.info.revision,
        },
    )
    .await
    else {
        panic!("claimed terminal expected")
    };
    execute(client, Command::InputTerminal { terminal: info.id, revision: info.revision,
        input: Input::Paste { text: concat!("awk 'BEGIN { for (i=0;i<20000;i++) printf ",
        "\"%06d continuous terminal output xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\\n\",i }'; ",
        "printf '%s%s\\n' 'load-' 'complete'").into() },
    }).await;
    execute(
        client,
        Command::InputTerminal {
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
        },
    )
    .await;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            screen.apply(1, subscription.next().await.unwrap()).unwrap();
            let content: String = screen
                .snapshot()
                .unwrap()
                .screen
                .rows
                .iter()
                .flat_map(|row| row.spans.iter().map(|span| span.text.as_str()))
                .collect();
            if content.contains("load-complete") {
                assert!(content.contains("019999"));
                break;
            }
        }
    })
    .await
    .expect("terminal workload deadline");
}
