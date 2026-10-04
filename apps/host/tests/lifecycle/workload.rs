use super::*;
use sailry_client::{Client, conversation::Projection};
use sailry_protocol::{
    Authentication, Command as NodeCommand, Effort, Output, Permission, ProviderId, SessionConfig,
    WorkMode, WorktreeId,
    conversation::{ModelApi, Provider, Status},
};
use serde_json::json;

#[path = "../../../../crates/node-runtime/tests/agent_support/mod.rs"]
#[allow(dead_code)]
mod models;

fn counters(pid: u32) -> (f64, u64) {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "time=,rss="])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    let mut fields = text.split_whitespace();
    let seconds = fields
        .next()
        .unwrap()
        .split(':')
        .fold(0., |seconds, value| {
            seconds * 60. + value.parse::<f64>().unwrap()
        });
    (seconds, fields.next().unwrap().parse().unwrap())
}

async fn execute(client: &Client, command: NodeCommand) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "measures a release Host with isolated model and PTY workloads; no performance thresholds"]
async fn history_and_terminal_load() {
    let directory = tempdir().unwrap();
    let controller = Node::start(directory.path().join("controller"))
        .await
        .unwrap();
    let profile = directory.path().join("host");
    let node = Node::start(&profile).await.unwrap();
    node.link().set_trust(controller.id(), true).await.unwrap();
    controller.link().set_trust(node.id(), true).await.unwrap();
    node.shutdown().await.unwrap();
    let mut host = Host::spawn(&profile);
    host.ready();
    let client = Client::new(controller.link().remote(host.address.clone().unwrap()));
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("source.txt"), "Tool output fixture\n".repeat(256)).unwrap();
    let Output::Project(project) = execute(
        &client,
        NodeCommand::RegisterProject {
            name: "Release workload".into(),
            path: root.to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected")
    };
    let server = models::Server::compaction(false).await;
    let provider = ProviderId::new();
    execute(
        &client,
        NodeCommand::PutProvider {
            expected_revision: 0,
            provider: Provider {
                options: None,
                id: provider,
                revision: 0,
                name: "Isolated model".into(),
                api: ModelApi::ChatCompletions,
                authentication: Authentication::ApiKey,
                endpoint: server.endpoint.clone(),
                enabled: true,
                models: vec![],
                default_model: String::new(),
                credential: None,
            },
        },
    )
    .await;
    let Output::Session(session) = execute(
        &client,
        NodeCommand::CreateSession {
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
    let before = counters(host.child.id());
    let started = Instant::now();
    let mut subscription = client.subscribe_conversation(session.id).await.unwrap();
    let mut conversation = Projection::new(client.target(), session.id, 1);
    conversation
        .apply(1, subscription.next().await.unwrap())
        .unwrap();
    for _ in 0..64 {
        let Output::QueuedTurn(turn) = execute(
            &client,
            NodeCommand::SubmitTurn {
                session: session.id,
                expected_revision: 1,
                message: "Preserve the task constraints and source evidence. "
                    .repeat(128)
                    .into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                conversation
                    .apply(1, subscription.next().await.unwrap())
                    .unwrap();
                if let Some(run) = conversation
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
                    break;
                }
            }
        })
        .await
        .expect("conversation workload deadline");
    }
    let after = counters(host.child.id());
    let elapsed = started.elapsed().as_secs_f64();
    let reading = Instant::now();
    let history = client
        .read_conversation(session.id, None, 100)
        .await
        .unwrap();
    assert_eq!(history.page.runs.len(), 64);
    let summaries = server
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| models::compaction::is_summary(request))
        .count();
    assert!(summaries > 0, "long history exercised context compaction");
    println!(
        "{}",
        json!({"workload":"history","turns":64,"summaries":summaries,
        "seconds":elapsed,"read_ms":reading.elapsed().as_secs_f64()*1000.,
        "host_cpu_seconds":after.0-before.0,"host_rss_kib":after.1,"entries":history.page.entries.len()})
    );
    drop(subscription);
    terminal(&client, &mut host, session.worktree).await;
    host.stop();
    controller.shutdown().await.unwrap();
}

async fn terminal(client: &Client, host: &mut Host, worktree: WorktreeId) {
    use sailry_protocol::terminal::*;
    let color = Rgb {
        red: 128,
        green: 128,
        blue: 128,
    };
    let Output::Terminal(info) = execute(
        client,
        NodeCommand::CreateTerminal(Launch {
            worktree,
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
    let mut subscription = client.subscribe_terminal(info.id).await.unwrap();
    let mut screen = sailry_client::terminal::Projection::new(client.target(), info.id, 1);
    screen.apply(1, subscription.next().await.unwrap()).unwrap();
    let before = counters(host.child.id());
    let started = Instant::now();
    let command = concat!(
        "awk 'BEGIN { for (i=0;i<20000;i++) printf ",
        "\"%06d continuous terminal output xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\\n\",i }'; ",
        "printf '%s%s\\n' 'load-' 'complete'"
    );
    execute(
        client,
        NodeCommand::InputTerminal {
            terminal: info.id,
            revision: info.revision,
            input: Input::Paste {
                text: command.into(),
            },
        },
    )
    .await;
    execute(
        client,
        NodeCommand::InputTerminal {
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
    let mut updates = 0;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            screen.apply(1, subscription.next().await.unwrap()).unwrap();
            updates += 1;
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
    let after = counters(host.child.id());
    println!(
        "{}",
        json!({"workload":"terminal","lines":20000,"updates":updates,
        "seconds":started.elapsed().as_secs_f64(),"host_cpu_seconds":after.0-before.0,"host_rss_kib":after.1})
    );
    execute(
        client,
        NodeCommand::CloseTerminal {
            worktree,
            terminal: info.id,
        },
    )
    .await;
}
