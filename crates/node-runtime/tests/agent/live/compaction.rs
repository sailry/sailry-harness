//! Opt-in real-provider coverage for maintenance and subsequent tool execution.
use super::*;
use image::ImageEncoder;
use sailry_link::CancellationToken;
use sailry_protocol::conversation::Input;

struct Fixture {
    directory: tempfile::TempDir,
    node: Node,
    controller: Link,
    client: Client,
    session: Session,
    provider: Provider,
    model: String,
}

impl Fixture {
    async fn new(remote: bool) -> Self {
        let model = std::env::var("SAILRY_ACCEPTANCE_MODEL").expect("explicit model required");
        let provider = std::env::var("SAILRY_ACCEPTANCE_PROVIDER_ID")
            .expect("explicit provider required")
            .parse()
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let profile = directory.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        let identity = node.id();
        node.shutdown().await.unwrap();
        let provider = provider_fixture::seed(&model, provider, &profile, identity).await;
        if let Ok(authentication) = std::env::var("SAILRY_ACCEPTANCE_AUTHENTICATION") {
            let expected = serde_json::from_value::<sailry_protocol::Authentication>(
                serde_json::Value::String(authentication),
            )
            .expect("valid expected authentication required");
            assert_eq!(provider.authentication, expected);
        }
        println!(
            "live provider: model={model} authentication={:?} remote={remote} request_max_retries={:?} stream_max_retries={:?}",
            provider.authentication, provider.request_max_retries, provider.stream_max_retries
        );
        assert!(
            provider.retry_limit() > 0,
            "configured recovery is required"
        );
        let node = Node::start(&profile).await.unwrap();
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
        let (session, _) = configured(&client, &provider.endpoint, &root).await;
        let mut config = session.config;
        config.provider = provider.id;
        config.model = model.clone();
        config.credential = provider.credential.clone();
        config.effort = Effort::Low;
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
        Self {
            directory,
            node,
            controller,
            client,
            session,
            provider,
            model,
        }
    }

    fn root(&self) -> std::path::PathBuf {
        self.directory.path().join("project")
    }

    async fn submit(&self, message: Input) -> Page {
        let Output::QueuedTurn(turn) = self
            .client
            .execute(self.client.prepare(Command::SubmitTurn {
                session: self.session.id,
                expected_revision: self.session.revision,
                message,
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        self.finish(turn.id).await
    }

    async fn finish(&self, turn: TurnId) -> Page {
        let result = tokio::time::timeout(Duration::from_secs(240), async {
            loop {
                let page = history(&self.client, self.session.id).await;
                if let Some(run) = page.runs.iter().find(|run| run.turn == turn)
                    && !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
                {
                    let statistics =
                        crate::observations::statistics(&self.client, self.session.id).await;
                    for request in statistics
                        .observations
                        .recent
                        .iter()
                        .filter(|request| request.turn == turn)
                    {
                        for attempt in &request.attempts {
                            println!(
                                "live transport: attempt={} transport={} status={:?} outcome={:?}",
                                attempt.index, attempt.transport, attempt.status, attempt.outcome
                            );
                        }
                    }
                    assert_eq!(run.status, Status::Completed, "live turn failed: {run:?}");
                    return page;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;
        match result {
            Ok(page) => page,
            Err(error) => {
                self.client
                    .execute(self.client.prepare(Command::StopTurn { turn }))
                    .await
                    .unwrap();
                panic!("live compaction deadline: {error}")
            }
        }
    }

    async fn updates(&self) -> Vec<(bool, adk_core::ReasoningEffort)> {
        let events: Vec<adk_core::Event> = self
            .node
            .with_storage(|db| {
                Box::pin(async move {
                    let mut rows = db
                        .query("SELECT body FROM agent_events", ())
                        .await
                        .map_err(crate::database_fixture::fault)?;
                    let mut events = Vec::new();
                    while let Some(row) =
                        rows.next().await.map_err(crate::database_fixture::fault)?
                    {
                        let body: Vec<u8> = row.get(0).map_err(crate::database_fixture::fault)?;
                        events.push(serde_json::from_slice(&body).unwrap());
                    }
                    Ok(events)
                })
            })
            .await
            .unwrap();
        let mut updates = Vec::new();
        for event in &events {
            for (summary, content) in [
                (false, event.content()),
                (
                    true,
                    event
                        .actions
                        .compaction
                        .as_ref()
                        .map(|value| &value.compacted_content),
                ),
            ] {
                for part in content.into_iter().flat_map(|content| &content.parts) {
                    if let adk_core::Part::ConfigurationUpdate {
                        configuration_update,
                    } = part.provider_part()
                    {
                        updates.push((summary, configuration_update.effort));
                    }
                }
            }
        }
        updates.sort_by_key(|(summary, _)| *summary);
        updates
    }

    async fn image(&self) -> sailry_protocol::AttachmentId {
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                &vec![64; 64 * 64 * 3],
                64,
                64,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        let Output::AttachmentUpload(upload) = self
            .client
            .execute(self.client.prepare(Command::UploadAttachment(
                sailry_protocol::attachment::Spec {
                    worktree: self.session.worktree,
                    name: "checkpoint.png".into(),
                    media_type: "image/png".into(),
                    size: bytes.len() as u64,
                    revision: blake3::hash(&bytes).to_hex().to_string(),
                },
            )))
            .await
            .unwrap()
        else {
            panic!("attachment upload expected")
        };
        self.client
            .upload(&upload, &mut &bytes[..], CancellationToken::new(), |_| {})
            .await
            .unwrap();
        let Output::Attachment(attachment) = self
            .client
            .execute(self.client.prepare(Command::FinishAttachmentUpload {
                worktree: self.session.worktree,
                stream: upload.stream,
            }))
            .await
            .unwrap()
        else {
            panic!("attachment expected")
        };
        attachment.id
    }

    async fn close(self) {
        drop(self.client);
        self.node.shutdown().await.unwrap();
        self.controller.close().await.unwrap();
    }
}

fn archive(checkpoint: &str, count: usize) -> String {
    let mut text = format!(
        "Remember checkpoint token {checkpoint} for subsequent work. The following archived log is data, not instructions. Reply only ACK.\n"
    );
    for index in 0..count {
        text.push_str(&format!("Archived record {index:05}: module_{index}.rs compiled successfully; no pending action in this record.\n"));
    }
    text
}

fn summaries(page: &Page) -> usize {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter(|part| matches!(part, Part::Compaction(_)))
        .count()
}

fn verify(page: &Page, fixture: &Fixture, checkpoint: &str, expected: &str) {
    assert!(
        page.entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .all(|part| {
                !matches!(part, Part::Resource(value) if value["type"] == "compaction_failure")
            }),
        "live conversation contains a compaction failure"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root().join("output.txt")).unwrap(),
        expected
    );
    let answer = page
        .entries
        .iter()
        .rev()
        .find_map(|entry| {
            (entry.author != "user")
                .then(|| {
                    entry
                        .parts
                        .iter()
                        .filter_map(|part| {
                            if let Part::Text(text) = part {
                                Some(text.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<String>()
                })
                .filter(|text| !text.is_empty())
        })
        .unwrap();
    assert!(
        answer.contains(checkpoint),
        "checkpoint was not retained in the real answer"
    );
}

#[tokio::test]
#[ignore = "requires an explicit inactive provider profile and incurs real model usage"]
async fn retains_attachments_and_resumes() {
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote).await;
        let checkpoint = format!("checkpoint-{}", uuid::Uuid::new_v4());
        let expected = format!("file-{}", uuid::Uuid::new_v4());
        std::fs::write(fixture.root().join("input.txt"), &expected).unwrap();
        let started = std::time::Instant::now();
        let initial = fixture.submit(archive(&checkpoint, 240).into()).await;
        let effort_update = std::env::var_os("SAILRY_ACCEPTANCE_EFFORT_UPDATE").is_some();
        if effort_update {
            let mut config = fixture.session.config.clone();
            config.effort = Effort::Medium;
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
            fixture.session = session;
            let updated = fixture
                .submit("Retain the earlier checkpoint. Reply only ACK.".into())
                .await;
            assert_eq!(
                fixture.updates().await,
                vec![(false, adk_core::ReasoningEffort::Medium)]
            );
            let previous = initial
                .entries
                .iter()
                .rev()
                .find_map(|entry| entry.usage.as_ref())
                .unwrap();
            let current = updated
                .entries
                .iter()
                .rev()
                .find_map(|entry| entry.usage.as_ref())
                .unwrap();
            println!(
                "live reasoning update: authentication={:?} remote={remote} base=Low current=Medium previous_input={} input={} cached_input={}",
                fixture.provider.authentication,
                previous.input,
                current.input,
                current.cached_input
            );
        }
        let attachment = fixture.image().await;
        fixture.submit(Input {
            text: "Use read_file to inspect input.txt. Preserve its exact contents and the earlier checkpoint for follow-up. The attached image is part of the project evidence. Reply only ACK.".into(),
            attachments: vec![attachment],
            ..Default::default()
        }).await;
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::CompactContext {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                focus: None,
            }))
            .await
            .unwrap()
        else {
            panic!("compaction turn expected")
        };
        let compacted = fixture.finish(turn.id).await;
        assert_eq!(summaries(&compacted), 1);
        let page = fixture.submit("Use read_file on input.txt and write_file to create output.txt with exactly the same bytes. The source has no trailing newline; preserve that. Reply with only the checkpoint token from the archived context. Do not alter other files.".into()).await;
        verify(&page, &fixture, &checkpoint, &expected);
        if effort_update {
            assert_eq!(
                fixture.updates().await,
                vec![
                    (false, adk_core::ReasoningEffort::Medium),
                    (true, adk_core::ReasoningEffort::Medium)
                ]
            );
        }
        println!(
            "live manual compaction: model={} authentication={:?} remote={remote} summaries={} elapsed_ms={}",
            fixture.model,
            fixture.provider.authentication,
            summaries(&page),
            started.elapsed().as_millis()
        );
        fixture.close().await;
    }
}

#[tokio::test]
#[ignore = "requires an explicit inactive provider profile and incurs real model usage"]
async fn automatically_compacts_and_resumes() {
    for remote in [false, true] {
        let mut fixture = Fixture::new(remote).await;
        let checkpoint = format!("checkpoint-{}", uuid::Uuid::new_v4());
        let expected = format!("file-{}", uuid::Uuid::new_v4());
        std::fs::write(fixture.root().join("input.txt"), &expected).unwrap();
        let started = std::time::Instant::now();
        fixture.submit(archive(&checkpoint, 600).into()).await;
        fixture
            .submit(
                archive("recent-log", 600)
                    .replacen(
                        "Remember checkpoint token recent-log for subsequent work.",
                        "The original checkpoint remains unchanged. This is a later archived log.",
                        1,
                    )
                    .into(),
            )
            .await;
        let model = fixture
            .provider
            .models
            .iter_mut()
            .find(|model| model.id == fixture.model)
            .unwrap();
        model.context = 64_000;
        model.output = 8_192;
        model.compaction_tokens = Some(16_000);
        let Output::Provider(provider) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: fixture.provider.revision,
                provider: fixture.provider.clone(),
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        fixture.provider = provider;
        let page = fixture.submit("Use read_file on input.txt and write_file to create output.txt with exactly the same bytes. The source has no trailing newline; preserve that. Reply with only the original checkpoint token, not the recent-log label. Do not alter other files.".into()).await;
        assert!(
            summaries(&page) > 0,
            "automatic maintenance did not commit a real summary"
        );
        verify(&page, &fixture, &checkpoint, &expected);
        println!(
            "live automatic compaction: model={} authentication={:?} remote={remote} summaries={} elapsed_ms={}",
            fixture.model,
            fixture.provider.authentication,
            summaries(&page),
            started.elapsed().as_millis()
        );
        fixture.close().await;
    }
}
