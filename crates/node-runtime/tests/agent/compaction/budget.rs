use super::*;
use image::ImageEncoder;
use sailry_link::CancellationToken;

fn evidence(fixture: &Fixture, lines: usize) {
    for index in 1..=6 {
        std::fs::write(
            fixture.root.join(format!("source-{index}.txt")),
            format!("Evidence {index}: project/src/module.rs 中文 🙂\n").repeat(lines),
        )
        .unwrap();
    }
}

fn summaries(page: &Page) -> usize {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter(|part| matches!(part, Part::Compaction(_)))
        .count()
}

async fn original_evidence(fixture: &Fixture, page: &Page) {
    let files: Vec<_> = tool_details(&fixture.client, page)
        .await
        .into_iter()
        .filter_map(|(name, detail)| {
            if name == plugin_tool("files", "read_file") {
                let Output::FileLines(file) =
                    serde_json::from_value(detail.result.unwrap()).unwrap()
                else {
                    panic!("file content expected")
                };
                Some(file)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(files.len(), 6);
    for file in files {
        assert_eq!(file.offset, 0);
        assert_eq!(file.next_offset, None);
        for (index, line) in file.lines.iter().enumerate() {
            assert_eq!(line.number, index + 1);
        }
        assert_eq!(
            file.lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<String>(),
            std::fs::read_to_string(fixture.root.join(file.path)).unwrap()
        );
    }
}

pub(super) async fn statistics(fixture: &Fixture) -> Statistics {
    let mut updates = fixture
        .client
        .subscribe_conversation(fixture.session.id)
        .await
        .unwrap();
    let Update::ConversationSnapshot(snapshot) = updates.next().await.unwrap() else {
        panic!("conversation snapshot expected")
    };
    snapshot.statistics
}

pub(super) async fn model(fixture: &Fixture, context: u32, output: u32, vision: bool) {
    let mut provider = configuration::snapshot(&fixture.client).await.providers[0].clone();
    if provider.models[0].context != context {
        provider.models[0].compaction_tokens = None;
    }
    provider.models[0].context = context;
    provider.models[0].output = output;
    provider.models[0].vision = vision;
    fixture
        .client
        .execute(fixture.client.prepare(Command::PutProvider {
            expected_revision: provider.revision,
            provider,
        }))
        .await
        .unwrap();
}

pub(super) async fn image(fixture: &Fixture) -> AttachmentId {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&[128, 64, 32, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    upload(fixture, "context.png", "image/png", &bytes).await
}

pub(super) async fn upload(
    fixture: &Fixture,
    name: &str,
    media_type: &str,
    bytes: &[u8],
) -> AttachmentId {
    let spec = sailry_protocol::attachment::Spec {
        worktree: fixture.session.worktree,
        name: name.into(),
        media_type: media_type.into(),
        size: bytes.len() as u64,
        revision: blake3::hash(bytes).to_hex().to_string(),
    };
    let Output::AttachmentUpload(upload) = fixture
        .client
        .execute(fixture.client.prepare(Command::UploadAttachment(spec)))
        .await
        .unwrap()
    else {
        panic!("attachment upload expected")
    };
    fixture
        .client
        .upload(&upload, &mut &bytes[..], CancellationToken::new(), |_| {})
        .await
        .unwrap();
    let Output::Attachment(attachment) = fixture
        .client
        .execute(fixture.client.prepare(Command::FinishAttachmentUpload {
            worktree: fixture.session.worktree,
            stream: upload.stream,
        }))
        .await
        .unwrap()
    else {
        panic!("attachment expected")
    };
    attachment.id
}

#[tokio::test]
async fn uses_reported_tokens() {
    for remote in [false, true] {
        let server = Server::loop_usage(vec![Some(84_000)], 100).await;
        let fixture = fixture(remote, &server).await;
        evidence(&fixture, 1000);
        model(&fixture, 1_050_000, 128_000, true).await;
        let attachment = image(&fixture).await;
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: Input {
                    text: "Inspect the image and preserve evidence from all six files".into(),
                    attachments: vec![attachment],
                    ..Default::default()
                },
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert_eq!(summaries(&page), 0);
        for text in ["Second recent turn", "Third recent turn"] {
            let (_, _, page) = submit(&fixture.client, fixture.session.id, text.into()).await;
            assert_eq!(summaries(&page), 0);
        }
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 9);
        assert!(
            requests
                .iter()
                .all(|request| !agent_support::compaction::is_summary(request))
        );
        assert!(
            requests[0]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| {
                    message["content"].as_array().is_some_and(|parts| {
                        parts.iter().any(|part| {
                            part["type"] == "image_url"
                                && part["image_url"]["url"]
                                    .as_str()
                                    .is_some_and(|url| url.starts_with("data:image/png;base64,"))
                        })
                    })
                })
        );
        let tool_bytes: usize = requests[6]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "tool")
            .map(|message| message["content"].to_string().len())
            .sum();
        assert!(tool_bytes > 128 * 1024);
        for index in 1..=6 {
            assert!(messages(&requests[8]).contains(&format!("Evidence {index}")));
        }
        let statistics = statistics(&fixture).await;
        assert_eq!(statistics.context_tokens, Some(84_010));
        assert_eq!(statistics.responses, 9);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn earlier_token_boundary() {
    for remote in [false, true] {
        for (context, output, configured, boundary) in [
            (1_050_000, 128_000, None, 225_000),
            (200_000, 64_000, None, 160_000),
            (1_050_000, 128_000, Some(200_000), 200_000),
        ] {
            let server = Server::loop_usage(
                vec![
                    Some(100),
                    Some(100),
                    Some(boundary * 3 / 5),
                    Some(boundary - 10),
                    Some(100),
                ],
                boundary,
            )
            .await;
            let fixture = fixture(remote, &server).await;
            model(&fixture, context, output, false).await;
            if let Some(tokens) = configured {
                let mut provider =
                    configuration::snapshot(&fixture.client).await.providers[0].clone();
                provider.models[0].compaction_tokens = Some(tokens);
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::PutProvider {
                        expected_revision: provider.revision,
                        provider,
                    }))
                    .await
                    .unwrap();
            }
            assert_eq!(
                statistics(&fixture).await.budget.unwrap().trigger_tokens,
                boundary as u64
            );
            evidence(&fixture, 1000);
            let (_, _, page) = submit(
                &fixture.client,
                fixture.session.id,
                "Preserve all evidence at the context boundary".into(),
            )
            .await;
            assert_eq!(summaries(&page), 1);
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| !agent_support::compaction::is_summary(request))
                    .count(),
                7
            );
            let summary = requests
                .iter()
                .find(|request| agent_support::compaction::is_summary(request))
                .unwrap();
            assert!(summary["tools"].as_array().is_none_or(Vec::is_empty));
            assert!(messages(summary).contains("Evidence 1"));
            original_evidence(&fixture, &page).await;
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn pending_tool_growth() {
    for remote in [false, true] {
        for large in [false, true] {
            let server = Server::loop_usage(vec![Some(100)], 100).await;
            let fixture = fixture(remote, &server).await;
            for index in 1..=6 {
                let text = if large && index == 3 {
                    format!(
                        "Evidence {index} original token evidence near the context boundary {}\n",
                        "a1 b2 c3 d4 e5 f6 g7 h8 ".repeat(2)
                    )
                    .repeat(1000)
                } else {
                    format!("Evidence {index} {}\n", "x".repeat(16 * 1024))
                };
                if large && index == 3 {
                    assert!(text.len() < sailry_protocol::MAX_FILE_BYTES);
                    assert!(
                        tiktoken_rs::o200k_base()
                            .unwrap()
                            .encode_with_special_tokens(&text)
                            .len()
                            > 24_000
                    );
                }
                std::fs::write(fixture.root.join(format!("source-{index}.txt")), text).unwrap();
            }
            let (_, _, page) = submit(
                &fixture.client,
                fixture.session.id,
                "Preserve all evidence while reading six files".into(),
            )
            .await;
            assert_eq!(summaries(&page), usize::from(large));
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 7 + usize::from(large));
            if large {
                let summary = requests
                    .iter()
                    .position(agent_support::compaction::is_summary)
                    .unwrap();
                assert!(messages(&requests[summary]).contains("Evidence 3"));
                assert!(messages(&requests[summary + 1]).contains("Context summary fixture"));
                assert!(
                    !messages(&requests[summary + 1])
                        .contains("original token evidence near the context boundary a1")
                );
            }
            original_evidence(&fixture, &page).await;
            assert_eq!(statistics(&fixture).await.context_tokens, Some(110));
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn retains_low_usage_summary() {
    for remote in [false, true] {
        let server =
            Server::loop_usage(vec![Some(100), Some(100), Some(26_000), Some(100)], 26_000).await;
        let fixture = fixture(remote, &server).await;
        evidence(&fixture, 600);
        let (_, _, page) = submit(
            &fixture.client,
            fixture.session.id,
            "Preserve the complete tool evidence".into(),
        )
        .await;
        assert_eq!(summaries(&page), 1);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 8);
        let summary = requests
            .iter()
            .position(agent_support::compaction::is_summary)
            .unwrap();
        assert!(messages(&requests[summary]).contains("Evidence 1"));
        assert!(
            requests[summary + 1..]
                .iter()
                .all(|request| !agent_support::compaction::is_summary(request))
        );
        assert!(messages(&requests[summary + 1]).contains("Context summary fixture"));
        original_evidence(&fixture, &page).await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn compacts_before_plain_followup() {
    for remote in [false, true] {
        let server = Server::summary(26_000).await;
        let fixture = fixture(remote, &server).await;
        let original = "Keep the original requirement ".repeat(120);
        let (_, _, before) = submit(&fixture.client, fixture.session.id, original.clone()).await;
        assert_eq!(summaries(&before), 0);
        let current = "Continue with this exact request";
        let (_, _, after) = submit(&fixture.client, fixture.session.id, current.into()).await;
        assert_eq!(summaries(&after), 1);
        assert_eq!(&after.entries[..before.entries.len()], before.entries);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        assert!(agent_support::compaction::is_summary(&requests[1]));
        assert!(messages(&requests[1]).contains(&original));
        assert!(messages(&requests[1]).contains(current));
        assert!(messages(&requests[2]).contains("Prepared summary"));
        assert!(messages(&requests[2]).contains(current));
        assert!(!messages(&requests[2]).contains(&"Conversation evidence ".repeat(200)));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
