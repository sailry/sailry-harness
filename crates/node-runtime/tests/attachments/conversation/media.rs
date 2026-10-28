use super::*;
use base64::Engine;

#[tokio::test]
async fn preserves_video_and_audio_references() {
    let fixture = Fixture::empty().await;
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
        provider.models[0]
            .capabilities
            .get_or_insert_with(Default::default)
            .vision = Some(false);
        provider.models[0].capabilities.as_mut().unwrap().tools = Some(true);
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
        for (attachment, (_, _, bytes)) in attachments.iter().zip(native::media()) {
            assert_eq!(download(&client, attachment).await, bytes);
            assert_eq!(
                attachment.spec.revision,
                blake3::hash(bytes).to_hex().to_string()
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
    for request in requests {
        let body = &request.body;
        let parts: Vec<_> = body["contents"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|message| message["parts"].as_array().unwrap())
            .collect();
        for (name, mime, bytes) in native::media() {
            assert!(
                parts
                    .iter()
                    .all(|part| part["inline_data"]["mime_type"] != mime)
            );
            assert!(body.to_string().contains(name));
            assert!(
                !body
                    .to_string()
                    .contains(&base64::engine::general_purpose::STANDARD.encode(bytes))
            );
            assert!(
                body.to_string()
                    .contains("Contents not included; use read_office")
            );
            assert!(
                body.to_string()
                    .contains("use an attachment-capable tool for other files")
            );
        }
        assert!(!body.to_string().contains("sailry-attachment://"));
    }
    fixture.close().await;
}
