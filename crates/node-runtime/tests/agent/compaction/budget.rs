use super::*;
use image::ImageEncoder;
use sailry_link::CancellationToken;

fn evidence(fixture: &Fixture) {
    for index in 1..=6 {
        std::fs::write(
            fixture.root.join(format!("source-{index}.txt")),
            format!("Evidence {index}: project/src/module.rs 中文 🙂\n").repeat(1000),
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

fn original_evidence(fixture: &Fixture, page: &Page) {
    let files: Vec<_> = page
        .entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::ToolResult { name, result, .. } if name == &plugin_tool("files", "read_file") => {
                let Output::FileContent(file) = serde_json::from_value(result.clone()).unwrap()
                else {
                    panic!("file content expected")
                };
                Some(file)
            }
            _ => None,
        })
        .collect();
    assert_eq!(files.len(), 6);
    for file in files {
        assert_eq!(
            file.text,
            std::fs::read_to_string(fixture.root.join(file.path)).unwrap()
        );
    }
}

async fn statistics(fixture: &Fixture) -> Statistics {
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

async fn model(fixture: &Fixture, context: u32, output: u32, vision: bool) {
    let mut provider = configuration::snapshot(&fixture.client).await.providers[0].clone();
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

async fn image(fixture: &Fixture) -> AttachmentId {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(&[128, 64, 32, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    let spec = sailry_protocol::attachment::Spec {
        worktree: fixture.session.worktree,
        name: "context.png".into(),
        media_type: "image/png".into(),
        size: bytes.len() as u64,
        revision: blake3::hash(&bytes).to_hex().to_string(),
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
        .upload_attachment(&upload, &mut &bytes[..], CancellationToken::new(), |_| {})
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
        let fixture = Fixture::new(remote, &server).await;
        evidence(&fixture);
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
        for (context, output, boundary) in
            [(1_050_000, 128_000, 840_000), (200_000, 64_000, 136_000)]
        {
            // Leave room for the pending tool result before reaching the boundary.
            // The fixture reports ten output tokens on each model response.
            let server = Server::loop_usage(
                vec![
                    Some(100),
                    Some(100),
                    Some(boundary - 20_000),
                    Some(boundary - 10),
                    Some(100),
                ],
                boundary,
            )
            .await;
            let fixture = Fixture::new(remote, &server).await;
            model(&fixture, context, output, false).await;
            evidence(&fixture);
            let (_, _, page) = submit(
                &fixture.client,
                fixture.session.id,
                "Preserve all evidence when the context reaches its token boundary".into(),
            )
            .await;
            assert_eq!(summaries(&page), 1);
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 8);
            assert!(
                requests[..4]
                    .iter()
                    .all(|request| !agent_support::compaction::is_summary(request))
            );
            // Three exchanges were available below the boundary, but stayed intact.
            for index in 1..=3 {
                assert!(messages(&requests[3]).contains(&format!("Evidence {index}")));
            }
            assert!(agent_support::compaction::is_summary(&requests[4]));
            let summary = messages(&requests[4]);
            assert!(summary.contains("Evidence 1"));
            assert!(summary.contains("Evidence 2"));
            assert!(!summary.contains("Evidence 3"));
            assert!(!summary.contains("Evidence 4"));
            let retained = requests[5]["messages"].as_array().unwrap();
            let calls: Vec<_> = retained
                .iter()
                .flat_map(|message| message["tool_calls"].as_array().into_iter().flatten())
                .map(|call| call["id"].as_str().unwrap())
                .collect();
            let results: Vec<_> = retained
                .iter()
                .filter(|message| message["role"] == "tool")
                .map(|message| message["tool_call_id"].as_str().unwrap())
                .collect();
            assert_eq!(calls, ["loop-3", "loop-4"]);
            assert_eq!(results, calls);
            // The summary's own high usage must not trigger another summary.
            for request in &requests[5..] {
                assert!(!agent_support::compaction::is_summary(request));
                let content = messages(request);
                assert!(content.contains("Context summary fixture"));
                assert!(content.contains("Evidence 3"));
                assert!(content.contains("Evidence 4"));
                assert!(!content.contains("Evidence 1"));
                assert!(!content.contains("Evidence 2"));
            }
            original_evidence(&fixture, &page);
            let statistics = statistics(&fixture).await;
            assert_eq!(statistics.context_tokens, Some(110));
            assert_eq!(statistics.responses, 8);
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
            let fixture = Fixture::new(remote, &server).await;
            for index in 1..=6 {
                let bytes = if large && index == 3 { 64 } else { 48 } * 1024;
                let mut text = format!("Evidence {index}\n");
                text.push_str(&"x".repeat(bytes - text.len()));
                std::fs::write(fixture.root.join(format!("source-{index}.txt")), text).unwrap();
            }
            let (_, _, page) = submit(
                &fixture.client,
                fixture.session.id,
                "Preserve all evidence while reading six files".into(),
            )
            .await;
            let summary_count = usize::from(large);
            assert_eq!(summaries(&page), summary_count);
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 7 + summary_count);
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| agent_support::compaction::is_summary(request))
                    .count(),
                summary_count
            );
            assert!(
                requests[..3]
                    .iter()
                    .all(|request| !agent_support::compaction::is_summary(request))
            );
            if large {
                assert!(agent_support::compaction::is_summary(&requests[3]));
                assert!(messages(&requests[3]).contains("Evidence 1"));
                assert!(!messages(&requests[3]).contains("Evidence 2"));
                assert!(!messages(&requests[3]).contains("Evidence 3"));
                for request in &requests[4..] {
                    let content = messages(request);
                    assert!(content.contains("Context summary fixture"));
                    assert!(content.contains("Evidence 2"));
                    assert!(content.contains("Evidence 3"));
                    assert!(!content.contains("Evidence 1"));
                }
            } else {
                assert!(messages(requests.last().unwrap()).contains("Evidence 1"));
            }
            original_evidence(&fixture, &page);
            let statistics = statistics(&fixture).await;
            let responses = 7 + summary_count as u64;
            assert_eq!(statistics.context_tokens, Some(110));
            assert_eq!(statistics.responses, responses);
            assert_eq!(
                statistics.usage,
                Some(Usage {
                    input: responses * 100,
                    output: responses * 10,
                    cached_input: 0,
                    reasoning: 0,
                })
            );
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn retains_low_usage_summary() {
    for remote in [false, true] {
        let server =
            Server::loop_usage(vec![Some(100), Some(100), Some(14_000), Some(100)], 14_000).await;
        let fixture = Fixture::new(remote, &server).await;
        evidence(&fixture);
        let (_, _, page) = submit(
            &fixture.client,
            fixture.session.id,
            "Preserve the complete tool exchanges".into(),
        )
        .await;
        assert_eq!(summaries(&page), 1);
        let summary = page
            .entries
            .iter()
            .find(|entry| {
                entry
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::Compaction(_)))
            })
            .unwrap();
        assert_eq!(summary.usage.as_ref().unwrap().input, 14_000);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 8);
        assert_eq!(
            requests
                .iter()
                .filter(|request| agent_support::compaction::is_summary(request))
                .count(),
            1
        );
        assert!(agent_support::compaction::is_summary(&requests[3]));
        assert!(messages(&requests[3]).contains("Evidence 1"));
        assert!(!messages(&requests[3]).contains("Evidence 2"));
        // Low subsequent usage must not restore ADK's original uncompressed prefix.
        for request in &requests[4..] {
            let content = messages(request);
            assert!(content.contains("Context summary fixture"));
            assert!(!content.contains("Evidence 1"));
            assert!(content.contains("Evidence 2"));
            assert!(content.contains("Evidence 3"));
            assert_eq!(request["messages"][0], requests[0]["messages"][0]);
        }
        assert!(messages(requests.last().unwrap()).contains("Evidence 6"));
        let statistics = statistics(&fixture).await;
        assert_eq!(statistics.context_tokens, Some(110));
        assert_eq!(statistics.responses, 8);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
