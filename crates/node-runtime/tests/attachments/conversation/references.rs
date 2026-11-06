use super::*;
use sailry_protocol::conversation::reference::{Reference, Target};

fn selection(id: AttachmentId) -> Reference {
    Reference {
        target: Target::Attachment(id),
        label: "Submit button".into(),
    }
}

#[tokio::test]
async fn binds_attachment_references() {
    let fixture = Fixture::empty().await;
    let server = server::Server::start(false).await;
    for client in fixture.clients() {
        let (session, _) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        let context = br#"{"kind":"browser_element","element":{"tag":"button","text":"Pay"}}"#;
        let json = upload(
            &client,
            fixture.worktree,
            "browser-element-1.json",
            "application/json",
            context,
        )
        .await;
        let image = upload(
            &client,
            fixture.worktree,
            "browser-element-1.png",
            "image/png",
            &png(),
        )
        .await;
        let outside = client
            .execute(client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: Input {
                    text: "Fix @Submit button".into(),
                    attachments: vec![json.id],
                    references: vec![selection(image.id)],
                },
            }))
            .await
            .unwrap_err();
        assert_eq!(outside.code, ErrorCode::InvalidRequest);
        let message = Input {
            text: "Fix @Submit button".into(),
            attachments: vec![image.id, json.id],
            references: vec![selection(image.id), selection(json.id)],
        };
        let turn = queue(&client, &session, message.clone()).await;
        let history = run(&client, &turn).await;
        assert_eq!(
            history.runs[0].status,
            Status::Completed,
            "{:?}",
            history.runs
        );
        let input = history
            .entries
            .iter()
            .find(|entry| entry.author == "user")
            .unwrap();
        for reference in &message.references {
            assert!(input.parts.contains(&Part::Reference(reference.clone())));
        }
        let request = server.requests.lock().unwrap().last().unwrap().to_string();
        assert!(request.contains("User-selected references"));
        assert!(request.contains("attachment targets identify attachments of this message"));
        assert!(request.contains(&json.id.to_string()));
        assert!(request.contains(&image.id.to_string()));
        assert!(request.contains("browser_element"));
    }
}
