use super::*;
use sailry_protocol::conversation::Image;
use serde_json::json;

fn request(session: SessionId, image: &Image) -> Command {
    Command::DownloadImage {
        session,
        image: image.clone(),
    }
}

async fn prepare(client: &Client, session: SessionId, image: &Image) -> Download {
    let admission = client
        .dispatch(client.prepare(request(session, image)))
        .await
        .unwrap();
    assert!(!admission.receipt.durable);
    let Output::AttachmentDownload(download) = admission.completion.await.unwrap().unwrap() else {
        panic!("image download expected")
    };
    assert_eq!(download.attachment, image.attachment);
    download
}

#[tokio::test]
async fn restores_canonical_images() {
    restores(false).await;
}

#[tokio::test]
async fn restores_assistant_images() {
    restores(true).await;
}

async fn restores(direct: bool) {
    let mut fixture = Fixture::start().await;
    std::fs::write(fixture.root.join("source.txt"), "Fixture input").unwrap();
    let server =
        server::Server::tools(vec![("read_file".into(), json!({"path":"source.txt"}))]).await;
    let mut records = Vec::new();
    for client in fixture.clients() {
        let (session, mut provider) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        provider.models[0].tools = true;
        provider.models[0].vision = !direct;
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        let turn = queue(&client, &session, "Read the fixture".into()).await;
        assert_eq!(run(&client, &turn).await.runs[0].status, Status::Completed);
        records.push((session.id, turn.id));
    }
    let profile = fixture.node.profile().to_owned();
    fixture.node.shutdown().await.unwrap();
    let bytes = png();
    // Add synthetic media to an already persisted fixture response while
    // its Node is stopped. This exercises canonical history without
    // depending on capture permissions or a second media database.
    {
        let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        for (_, turn) in &records {
            let rows = {
                let mut query = db
                    .prepare(
                        "SELECT sequence,body FROM agent_events WHERE turn=?1 ORDER BY sequence",
                    )
                    .unwrap();
                query
                    .query_map([turn.to_string()], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
                    })
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
            };
            let mut inserted = false;
            for (sequence, body) in rows {
                let mut event: adk_core::Event = serde_json::from_slice(&body).unwrap();
                let Some(content) = event.llm_response.content.as_mut() else {
                    continue;
                };
                if direct {
                    if event.author == "user"
                        || !content.parts.iter().any(|part| part.text().is_some())
                    {
                        continue;
                    }
                    content.parts.push(adk_core::Part::InlineData {
                        mime_type: "image/png".into(),
                        data: bytes.clone(),
                        uri: None,
                        annotations: None,
                    });
                    content.parts.push(adk_core::Part::Text {
                        text: "After the image".into(),
                    });
                } else {
                    let Some(response) = content.parts.iter_mut().find_map(|part| {
                        if let adk_core::Part::FunctionResponse {
                            function_response, ..
                        } = part
                        {
                            Some(function_response)
                        } else {
                            None
                        }
                    }) else {
                        continue;
                    };
                    response.inline_data.push(adk_core::InlineDataPart {
                        mime_type: "image/png".into(),
                        data: bytes.clone(),
                        uri: None,
                        annotations: None,
                    });
                }
                db.execute(
                    "UPDATE agent_events SET body=?2 WHERE sequence=?1",
                    rusqlite::params![sequence, serde_json::to_vec(&event).unwrap()],
                )
                .unwrap();
                inserted = true;
                break;
            }
            assert!(inserted, "fixture must contain a canonical response");
        }
        assert_eq!(count(&db, "attachments"), 0);
    }
    fixture.node = Node::start(&profile).await.unwrap();
    let requests = server.requests.lock().unwrap().len();
    let clients = fixture.clients();
    let mut images = Vec::new();
    for (client, (session, _)) in clients.iter().zip(&records) {
        let history = page(client, *session).await;
        let image = history
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .find_map(|part| match part {
                Part::ToolResult { images, .. } => images.first().cloned(),
                Part::Image(image) => Some(image.clone()),
                _ => None,
            })
            .expect("persisted image projection");
        if direct {
            let entry = history
                .entries
                .iter()
                .find(|entry| entry.id == image.entry)
                .unwrap();
            assert!(matches!(entry.parts.first(), Some(Part::Text(_))));
            assert_eq!(
                entry.parts.last(),
                Some(&Part::Text("After the image".into()))
            );
            assert_eq!(image.index, 0);
        }
        assert_eq!(image.attachment.spec.worktree, fixture.worktree);
        assert_eq!(image.attachment.spec.media_type, "image/png");
        assert_eq!(image.attachment.spec.size, bytes.len() as u64);
        assert_eq!(
            image.attachment.spec.revision,
            blake3::hash(&bytes).to_hex().to_string()
        );
        let download = prepare(client, *session, &image).await;
        let mut received = Vec::new();
        client
            .download_attachment(&download, &mut received, CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert_eq!(received, bytes);
        let cancelled = prepare(client, *session, &image).await;
        client
            .execute(client.prepare(Command::CancelFileTransfer {
                stream: cancelled.stream,
            }))
            .await
            .unwrap();
        assert!(
            client
                .download_attachment(
                    &cancelled,
                    &mut Vec::new(),
                    CancellationToken::new(),
                    |_| {}
                )
                .await
                .is_err()
        );
        let mut altered = image.clone();
        altered.attachment.spec.revision = "forged".into();
        assert_eq!(
            client
                .execute(client.prepare(request(*session, &altered)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        images.push(image);
    }
    assert_ne!(images[0].attachment.id, images[1].attachment.id);
    for (index, client) in clients.iter().enumerate() {
        assert_eq!(
            client
                .execute(client.prepare(request(records[1 - index].0, &images[index])))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
    let download = prepare(&clients[0], records[0].0, &images[0]).await;
    assert!(
        clients[1]
            .download_attachment(&download, &mut Vec::new(), CancellationToken::new(), |_| {})
            .await
            .is_err()
    );
    let mut received = Vec::new();
    clients[0]
        .download_attachment(&download, &mut received, CancellationToken::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(received, bytes);
    fixture.node.shutdown().await.unwrap();
    fixture.node = Node::start(&profile).await.unwrap();
    for (client, ((session, _), expected)) in
        fixture.clients().iter().zip(records.iter().zip(&images))
    {
        let history = page(client, *session).await;
        assert!(
            history
                .entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| match part {
                    Part::ToolResult { images, .. } => images.contains(expected),
                    Part::Image(image) => image == expected,
                    _ => false,
                })
        );
        let download = prepare(client, *session, expected).await;
        let mut received = Vec::new();
        client
            .download_attachment(&download, &mut received, CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert_eq!(received, bytes);
    }
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    assert_eq!(count(&db, "attachments"), 0);
    assert!(
        !profile
            .join("attachments")
            .join(images[0].attachment.id.to_string())
            .exists()
    );
    drop(db);
    assert_eq!(server.requests.lock().unwrap().len(), requests);
    if direct {
        for (client, (session, _)) in fixture.clients().iter().zip(&records) {
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::QueueTurn {
                    session: *session,
                    expected_revision: 1,
                    message: "Continue from the image".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("queued turn expected")
            };
            let history = run(client, &turn).await;
            assert_eq!(
                history.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                history.runs
            );
            assert!(
                history
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| matches!(part, Part::Image(_)))
            );
        }
        let requests = server.requests.lock().unwrap();
        let continuation = &requests[requests.len() - 1];
        assert!(continuation.to_string().contains("does not support images"));
        assert!(!continuation.to_string().contains("data:image/"));
    }
    fixture.close().await;
}
