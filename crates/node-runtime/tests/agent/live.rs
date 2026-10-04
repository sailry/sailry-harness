//! Opt-in billable model acceptance using caller-selected credentials and isolated files.
use super::*;

#[derive(serde::Deserialize)]
struct Source {
    endpoint: String,
    model: String,
    secret: Secret,
}

#[tokio::test]
#[ignore = "requires SAILRY_ACCEPTANCE_PROVIDER JSON and incurs model usage"]
async fn completes_file_task() {
    let path = std::env::var_os("SAILRY_ACCEPTANCE_PROVIDER").expect("provider source required");
    let source: Source = serde_json::from_slice(&std::fs::read(path).unwrap())
        .expect("valid provider source required");
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let expected = uuid::Uuid::new_v4().to_string();
        std::fs::write(root.join("input.txt"), &expected).unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let (session, mut provider) = configured(&client, &source.endpoint, &root).await;
        provider.default_model = source.model.clone();
        provider.models = vec![Model {
            id: source.model.clone(),
            context: 128000,
            output: 8192,
            vision: false,
            tools: true,
            reasoning: true,
            web_search: false,
            generates: vec![],
            efforts: vec![Effort::Low],
            custom_efforts: false,
            default_effort: Effort::Low,
        }];
        let Output::Provider(provider) = client
            .execute(client.prepare(Command::SaveProvider {
                expected_revision: provider.revision,
                provider,
                secret: Some(source.secret.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        let mut config = session.config.clone();
        config.model = source.model.clone();
        config.effort = Effort::Low;
        config.credential = provider.credential.clone();
        config.permission = Permission::Full;
        let Output::Session(session) = client
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
        let Output::QueuedTurn(turn) = client.execute(client.prepare(Command::SubmitTurn {
            session: session.id, expected_revision: session.revision,
            message: "Use read_file to read input.txt, then write_file to create output.txt containing exactly the same bytes. Do not change other files. Reply only with the file contents after the write succeeds.".into(),
        })).await.unwrap() else { panic!("turn expected") };
        let started = std::time::Instant::now();
        let page = loop {
            let page = history(&client, session.id).await;
            if page.runs.iter().any(|run| {
                run.turn == turn.id
                    && !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
            }) {
                break page;
            }
            if started.elapsed() > Duration::from_secs(180) {
                client
                    .execute(client.prepare(Command::StopTurn { turn: turn.id }))
                    .await
                    .unwrap();
                break finished(&client, session.id, turn.id).await;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        };
        let status = page.runs.last().unwrap().status;
        let output = std::fs::read_to_string(root.join("output.txt"));
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        assert_eq!(status, Status::Completed, "live model task failed");
        assert_eq!(output.unwrap(), expected, "actual file outcome differs");
        if !page.entries.iter().any(|entry| {
            entry.author != "user" && entry.parts.iter().any(|part| matches!(part, Part::Text(_)))
        }) {
            eprintln!(
                "Live model returned no visible answer: {}",
                serde_json::to_string(&page.entries).unwrap()
            );
        }
        super::evaluation::response(&page, &expected);
        println!(
            "Live model {}: file outcome and response verified in {} ms",
            if remote { "remote" } else { "local" },
            started.elapsed().as_millis()
        );
    }
}
