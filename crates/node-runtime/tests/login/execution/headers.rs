use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keeps_transient_headers_out_of_cache() {
    for remote in [false, true] {
        for api in [ModelApi::Responses, ModelApi::ChatCompletions] {
            let server = Server::start_with_request(ModelApi::Anthropic, |request| {
                if exchange(request) {
                    Reply::Json(exchanged(request, Authentication::Copilot, true))
                } else if matches!(request.path.as_str(), "/responses" | "/chat/completions") {
                    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
                    let tools = body
                        .get("tools")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|tools| !tools.is_empty());
                    generation(request, Authentication::Copilot, tools)
                } else {
                    standard(request)
                }
            })
            .await;
            let fixture = Fixture::packages(remote, &server, &["files"]).await;
            let provider = signed_in(&fixture, Authentication::Copilot, api).await;
            let session = session(&fixture, &provider).await;
            let attachment = image(&fixture.client, session.worktree).await;
            let Output::QueuedTurn(turn) = execute(
                &fixture.client,
                Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: sailry_protocol::conversation::Input {
                        text: "Inspect this pixel".into(),
                        attachments: vec![attachment],
                        references: vec![],
                    },
                },
            )
            .await
            else {
                panic!("turn expected")
            };
            finished(&fixture.client, &turn, Status::Completed).await;
            let Output::QueuedTurn(compact) = execute(
                &fixture.client,
                Command::CompactContext {
                    session: session.id,
                    expected_revision: session.revision,
                    focus: None,
                },
            )
            .await
            else {
                panic!("compaction turn expected")
            };
            finished(&fixture.client, &compact, Status::Completed).await;
            let (_, turn) = submit(&fixture.client, &session).await;
            finished(&fixture.client, &turn, Status::Completed).await;
            {
                let requests = server.requests.lock().unwrap();
                let generated: Vec<_> = requests
                    .iter()
                    .filter(|request| {
                        matches!(request.path.as_str(), "/responses" | "/chat/completions")
                    })
                    .collect();
                assert_eq!(generated.len(), 5);
                for (index, request) in generated.iter().enumerate() {
                    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
                    let messages = body[if api == ModelApi::Responses {
                        "input"
                    } else {
                        "messages"
                    }]
                    .as_array()
                    .unwrap();
                    let has_image = messages
                        .iter()
                        .filter_map(|message| message["content"].as_array())
                        .flatten()
                        .any(|part| {
                            matches!(part["type"].as_str(), Some("input_image" | "image_url"))
                        });
                    assert_eq!(has_image, index < 2, "{api:?}, remote={remote}: {body}");
                    assert_eq!(
                        request.headers.contains_key("copilot-vision-request"),
                        has_image,
                        "{api:?}, remote={remote}: {:?}",
                        request.headers
                    );
                }
                let ids: std::collections::BTreeSet<_> = generated
                    .iter()
                    .map(|request| request.headers["x-request-id"].as_str())
                    .collect();
                assert_eq!(ids.len(), generated.len());
                assert_eq!(
                    generated
                        .iter()
                        .map(|request| request.headers["x-initiator"].as_str())
                        .collect::<Vec<_>>(),
                    ["user", "agent", "user", "user", "agent"]
                );
            }
            fixture.close().await;
        }
    }
}

async fn image(client: &Client, worktree: WorktreeId) -> AttachmentId {
    let bytes = base64::engine::general_purpose::STANDARD.decode(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j3ioAAAAASUVORK5CYII="
    ).unwrap();
    let Output::AttachmentUpload(upload) = execute(
        client,
        Command::UploadAttachment(sailry_protocol::attachment::Spec {
            worktree,
            name: "pixel.png".into(),
            media_type: "image/png".into(),
            size: bytes.len() as u64,
            revision: blake3::hash(&bytes).to_hex().to_string(),
        }),
    )
    .await
    else {
        panic!("attachment upload expected")
    };
    client
        .upload(
            &upload,
            &mut bytes.as_slice(),
            sailry_link::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    let Output::Attachment(attachment) = execute(
        client,
        Command::FinishAttachmentUpload {
            worktree,
            stream: upload.stream,
        },
    )
    .await
    else {
        panic!("attachment expected")
    };
    attachment.id
}
