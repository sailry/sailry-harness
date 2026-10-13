use super::*;

use base64::Engine;

#[tokio::test]
async fn preserves_unsupported_pdf_history() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let fixture = Fixture::start().await;
        let server = server::Server::start(false).await;
        for client in fixture.clients() {
            let (session, _) = session(&client, fixture.worktree, &server.endpoint, api).await;
            let bytes = pdf();
            let document = upload(
                &client,
                fixture.worktree,
                "report.pdf",
                "application/pdf",
                &bytes,
            )
            .await;
            let turn = queue(&client, &session, input("Read the PDF", &[&document])).await;
            let failed = run(&client, &turn).await;
            assert_eq!(failed.runs[0].status, Status::Failed);
            let error = failed.runs[0].error.as_ref().unwrap();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert_eq!(
                error.message,
                "attachment requires tools or a supported native model input"
            );
            let original = failed
                .entries
                .iter()
                .find(|entry| entry.author == "user")
                .unwrap()
                .clone();
            assert!(original.parts.contains(&Part::Attachment(document.clone())));
            let next = queue(&client, &session, "Continue without reading the PDF".into()).await;
            let continued = run(&client, &next).await;
            assert_eq!(continued.runs.last().unwrap().status, Status::Completed);
            assert!(continued.entries.contains(&original));
            assert_eq!(download(&client, &document).await, bytes);
        }
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        for request in requests {
            let serialized = request.to_string();
            assert!(serialized.contains("Continue without reading the PDF"));
            assert!(serialized.contains("report.pdf"));
            assert!(
                serialized.contains(
                    "Contents not included; the current model cannot read this attachment"
                )
            );
            assert!(!serialized.contains("data:application/pdf;base64,"));
            assert!(!serialized.contains("sailry-attachment://"));
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn materializes_inputs() {
    let bytes = png();
    let pdf_bytes = pdf();
    let data_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let mut fixture = Fixture::start().await;
        let server = server::Server::start(false).await;
        let mut records = Vec::new();
        let text = "完整内容 🙂\n".repeat(400);
        for client in fixture.clients() {
            let (session, mut provider) =
                session(&client, fixture.worktree, &server.endpoint, api).await;
            provider.models[0].capabilities.as_mut().unwrap().tools = Some(true);
            client
                .execute(client.prepare(Command::PutProvider {
                    expected_revision: provider.revision,
                    provider,
                }))
                .await
                .unwrap();
            let image = upload(
                &client,
                fixture.worktree,
                "截图 🙂.bin",
                "application/octet-stream",
                &bytes,
            )
            .await;
            let document = upload(
                &client,
                fixture.worktree,
                "内容.txt",
                "text/plain",
                text.as_bytes(),
            )
            .await;
            let pdf = upload(
                &client,
                fixture.worktree,
                "报告.bin",
                "application/octet-stream",
                &pdf_bytes,
            )
            .await;
            let turn = queue(&client, &session, input("", &[&image, &document, &pdf])).await;
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
            assert_eq!(
                input.parts,
                vec![
                    Part::Attachment(image.clone()),
                    Part::Attachment(document.clone()),
                    Part::Attachment(pdf.clone())
                ]
            );
            for format in [
                sailry_client::conversation::export::Format::Markdown,
                sailry_client::conversation::export::Format::Html,
            ] {
                let exported = client
                    .export_conversation(
                        session.id,
                        "Attachments",
                        format,
                        "assets",
                        &CancellationToken::new(),
                    )
                    .await
                    .unwrap();
                assert_eq!(exported.attachments.len(), 3);
                for attachment in [&image, &document, &pdf] {
                    assert!(exported.attachments.contains(attachment));
                    assert!(exported.content.contains(
                        &sailry_client::conversation::export::attachment_name(attachment)
                    ));
                }
                assert_eq!(download(&client, &document).await, text.as_bytes());
                assert_eq!(download(&client, &image).await, bytes);
                assert_eq!(download(&client, &pdf).await, pdf_bytes);
            }
            assert_eq!(
                client
                    .execute(discard(&client, &image))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            let db =
                rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
                    .unwrap();
            let body: Vec<u8> = db.query_row("SELECT body FROM agent_events WHERE turn=?1 AND json_extract(body,'$.author')='user'", [turn.id.to_string()], |row| row.get(0)).unwrap();
            assert!(body.len() < 2048);
            assert!(
                String::from_utf8(body)
                    .unwrap()
                    .contains("sailry-attachment://")
            );
            records.push((session, turn, history, pdf));
        }
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        fixture.node = Node::start(&profile).await.unwrap();
        for (client, (session, turn, history, pdf)) in fixture.clients().into_iter().zip(records) {
            assert_eq!(page(&client, session.id).await, history);
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
            assert!(
                page(&client, branch.id)
                    .await
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| *part == Part::Attachment(pdf.clone()))
            );
            let turn = queue(&client, &branch, "Continue from the attachment".into()).await;
            let history = run(&client, &turn).await;
            assert_eq!(
                history.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                history.runs
            );
        }
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        for request in requests {
            let messages = match api {
                ModelApi::ChatCompletions => &request["messages"],
                ModelApi::Responses => &request["input"],
                _ => unreachable!(),
            };
            let users: Vec<_> = messages
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| message["role"] == "user")
                .collect();
            let content = users
                .iter()
                .find_map(|message| message["content"].as_array())
                .expect("image content expected");
            assert_eq!(
                content[0]["type"],
                if api == ModelApi::Responses {
                    "input_image"
                } else {
                    "image_url"
                }
            );
            let url = if api == ModelApi::Responses {
                &content[0]["image_url"]
            } else {
                &content[0]["image_url"]["url"]
            };
            assert_eq!(url, &data_url);
            let parts = users
                .iter()
                .filter_map(|message| message["content"].as_array())
                .flatten()
                .collect::<Vec<_>>();
            assert!(
                !parts
                    .iter()
                    .any(|part| { part["type"] == "input_file" || part["type"] == "file" })
            );
            assert!(
                request
                    .to_string()
                    .contains("Contents not included; use read_office")
            );
            assert!(request.to_string().contains("报告.bin"));
            assert!(!request.to_string().contains("data:application/pdf;base64,"));
            let expected = format!("Attachment: 内容.txt\n{text}");
            assert!(users.iter().any(|message| {
                message["content"] == expected
                    || message["content"]
                        .as_array()
                        .is_some_and(|parts| parts.iter().any(|part| part["text"] == expected))
            }));
            assert!(!request.to_string().contains("sailry-attachment://"));
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn rejects_damaged_content() {
    let fixture = Fixture::start().await;
    let server = server::Server::start(false).await;
    for client in fixture.clients() {
        for data in [&b"invalid\xff"[..], &b"binary\0text"[..], &b"original"[..]] {
            let (session, _) = session(
                &client,
                fixture.worktree,
                &server.endpoint,
                ModelApi::ChatCompletions,
            )
            .await;
            let file = upload(&client, fixture.worktree, "input.txt", "text/plain", data).await;
            if data == b"original" {
                std::fs::write(
                    fixture
                        .node
                        .profile()
                        .join("attachments")
                        .join(file.id.to_string()),
                    b"modified",
                )
                .unwrap();
            }
            let turn = queue(&client, &session, input("Inspect", &[&file])).await;
            let history = run(&client, &turn).await;
            let run = &history.runs[0];
            assert_eq!(run.status, Status::Failed);
            assert_eq!(
                run.error.as_ref().unwrap().code,
                if data == b"original" {
                    ErrorCode::RevisionConflict
                } else {
                    ErrorCode::InvalidRequest
                }
            );
        }
    }
    assert!(server.requests.lock().unwrap().is_empty());
    fixture.close().await;
}

#[tokio::test]
async fn restores_rewound_inputs() {
    let fixture = Fixture::start().await;
    let server = server::Server::start(false).await;
    for client in fixture.clients() {
        let (session, _) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        let file = upload(
            &client,
            fixture.worktree,
            "retained.txt",
            "text/plain",
            b"retained input",
        )
        .await;
        let turn = queue(&client, &session, input("", &[&file])).await;
        let original = run(&client, &turn).await;
        let Output::Session(retained) = client
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
        let Output::Rewound(rewind) = client
            .execute(client.prepare(Command::RewindConversation {
                session: session.id,
                through: None,
                expected_head: turn.id,
                expected_revision: original.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("rewind expected")
        };
        assert!(page(&client, session.id).await.entries.is_empty());
        assert!(rewind.backup.is_none());
        assert_eq!(page(&client, retained.id).await.entries, original.entries);
        let next = queue(&client, &retained, "Continue the fork".into()).await;
        let history = run(&client, &next).await;
        assert_eq!(
            history.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            history.runs
        );
        assert_eq!(
            client
                .execute(discard(&client, &file))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }
    assert_eq!(server.requests.lock().unwrap().len(), 4);
    fixture.close().await;
}
