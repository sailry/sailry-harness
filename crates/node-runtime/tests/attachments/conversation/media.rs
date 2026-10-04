use super::*;
use base64::Engine;

#[tokio::test]
async fn preserves_video_and_audio_references() {
    let fixture = Fixture::start().await;
    let server = provider::Server::start(ModelApi::Gemini, provider::Reply::Text).await;
    for client in fixture.clients() {
        let (session, mut provider) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::Gemini,
        )
        .await;
        let mut attachments = Vec::new();
        for (name, _, bytes) in native::media() {
            attachments.push(
                upload(
                    &client,
                    fixture.worktree,
                    name,
                    "application/octet-stream",
                    bytes,
                )
                .await,
            );
        }
        let turn = queue(
            &client,
            &session,
            input("Inspect media", &attachments.iter().collect::<Vec<_>>()),
        )
        .await;
        provider.models[0].vision = false;
        provider.models[0].tools = true;
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        assert_eq!(run(&client, &turn).await.runs[0].status, Status::Completed);
        let Output::Session(branch) = client
            .execute(client.prepare(Command::ForkConversation {
                session: session.id,
                through: turn.id,
                expected_revision: session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected")
        };
        let history = page(&client, branch.id).await;
        for attachment in &attachments {
            assert!(
                history
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| *part == Part::Attachment(attachment.clone()))
            );
        }
        let next = queue(&client, &branch, "Inspect again".into()).await;
        assert_eq!(
            run(&client, &next).await.runs.last().unwrap().status,
            Status::Completed
        );
    }
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 4);
    for pair in requests.as_chunks::<2>().0 {
        for (index, request) in pair.iter().enumerate() {
            let body = &request.body;
            let parts: Vec<_> = body["contents"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|message| message["parts"].as_array().unwrap())
                .collect();
            for (_, mime, bytes) in native::media() {
                let inline = parts
                    .iter()
                    .find(|part| part["inlineData"]["mimeType"] == mime);
                if index == 0 || mime.starts_with("audio/") {
                    assert_eq!(
                        inline.unwrap()["inlineData"]["data"],
                        base64::engine::general_purpose::STANDARD.encode(bytes)
                    );
                } else {
                    assert!(inline.is_none());
                    assert!(
                        body.to_string().contains(
                            "Contents not included; an attachment-capable tool is required"
                        )
                    );
                }
            }
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn preserves_unsupported_media_history() {
    let fixture = Fixture::start().await;
    let server = provider::Server::start(ModelApi::Anthropic, provider::Reply::Text).await;
    for client in fixture.clients() {
        let (session, mut provider) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::Anthropic,
        )
        .await;
        let mut attachments = Vec::new();
        for (name, _, bytes) in native::media() {
            attachments.push(upload(&client, fixture.worktree, name, "text/plain", bytes).await);
        }
        let turn = queue(
            &client,
            &session,
            input("Inspect", &attachments.iter().collect::<Vec<_>>()),
        )
        .await;
        let failed = run(&client, &turn).await;
        assert_eq!(failed.runs.last().unwrap().status, Status::Failed);
        for attachment in &attachments {
            assert!(
                failed
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| *part == Part::Attachment(attachment.clone()))
            );
        }
        provider.models[0].tools = true;
        client
            .execute(client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        let next = queue(&client, &session, "Use the files".into()).await;
        assert_eq!(
            run(&client, &next).await.runs.last().unwrap().status,
            Status::Completed
        );
    }
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    for request in requests.iter() {
        let body = request.body.to_string();
        assert!(body.contains("Contents not included; an attachment-capable tool is required"));
        for (_, _, bytes) in native::media() {
            assert!(!body.contains(&base64::engine::general_purpose::STANDARD.encode(bytes)));
        }
    }
    fixture.close().await;
}
