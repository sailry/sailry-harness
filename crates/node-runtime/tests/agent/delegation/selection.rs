use super::*;
use sailry_protocol::conversation::{
    Input,
    reference::{Reference, Target},
};

#[tokio::test]
async fn restores_frozen_input() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("source.txt"), "Selected workspace contents").unwrap();
        let profile = directory.path().join("node");
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
            controller.handle().remote(address.clone())
        } else {
            node.local()
        });
        let parent = Server::start(false).await;
        let child = Server::tools(vec![(
            plugin_tool("files", "read_file"),
            json!({"path":"source.txt"}),
        )])
        .await;
        let (session, _, mut role) = setup(&client, &root, &parent, &child).await;
        let bytes = b"Selected attachment contents";
        let Output::AttachmentUpload(upload) = client
            .execute(client.prepare(Command::UploadAttachment(
                sailry_protocol::attachment::Spec {
                    worktree: session.worktree,
                    name: "input.txt".into(),
                    media_type: "text/plain".into(),
                    size: bytes.len() as u64,
                    revision: blake3::hash(bytes).to_hex().to_string(),
                },
            )))
            .await
            .unwrap()
        else {
            panic!("upload expected");
        };
        client
            .upload_attachment(
                &upload,
                &mut &bytes[..],
                sailry_link::CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        let Output::Attachment(attachment) = client
            .execute(client.prepare(Command::FinishAttachmentUpload {
                worktree: session.worktree,
                stream: upload.stream,
            }))
            .await
            .unwrap()
        else {
            panic!("attachment expected");
        };
        let references = vec![
            Reference {
                target: Target::File("source.txt".into()),
                label: "source.txt".into(),
            },
            Reference {
                target: Target::Agent(role.reference()),
                label: "@review".into(),
            },
            Reference {
                target: Target::Session(session.id),
                label: "Related session".into(),
            },
        ];
        let input = Input {
            text: "Inspect selected inputs".into(),
            references: references.clone(),
            attachments: vec![attachment.id],
        };
        let mut stale = input.clone();
        if let Target::Agent(role) = &mut stale.references[1].target {
            role.revision += 1;
        }
        assert!(
            client
                .execute(client.prepare(Command::QueueTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: stale
                }))
                .await
                .is_err()
        );
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: input.clone(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected");
        };
        role.instructions = "Changed catalog instructions".into();
        client
            .execute(client.prepare(Command::PutRole {
                expected_revision: role.revision,
                role,
            }))
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::SetSessionRoles {
                session: session.id,
                expected_revision: session.revision,
                roles: vec![],
            }))
            .await
            .unwrap();
        let mut edited = input;
        edited.text = "Edited request".into();
        client
            .execute(client.prepare(Command::EditQueuedTurn {
                turn: turn.id,
                expected_revision: 1,
                message: edited,
            }))
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::StartQueuedTurn { turn: turn.id }))
            .await
            .unwrap();
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap()
                .status,
            Status::Completed,
            "{page:?}"
        );
        let saved: Vec<_> = page
            .entries
            .iter()
            .filter(|entry| entry.author == "user")
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::Reference(reference) => Some(reference.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(saved, references);
        assert!(page.entries.iter().any(|entry| entry.author == "user"
            && entry.parts.contains(&Part::Text("Edited request".into()))));
        let children = children(&client, session.id, 1).await;
        let Output::Conversation(history) = client
            .execute(client.prepare(Command::ReadConversation {
                session: children[0].id,
                before: None,
                limit: 50,
            }))
            .await
            .unwrap()
        else {
            panic!("child history expected");
        };
        assert!(history.page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::Reference(reference) if reference.target == Target::File("source.txt".into()))));
        let requests = child.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0]
                .to_string()
                .contains("Frozen review instructions")
        );
        assert!(
            requests[0]
                .to_string()
                .contains("Selected attachment contents")
        );
        assert!(
            !requests[0]
                .to_string()
                .contains("Changed catalog instructions")
        );
        assert!(
            requests[1]
                .to_string()
                .contains("Selected workspace contents")
        );
        assert_eq!(parent.requests.lock().unwrap().len(), 1);
        let Output::Session(branch) = client
            .execute(client.prepare(Command::ForkConversation {
                session: session.id,
                through: turn.id,
                expected_revision: session.revision + 1,
            }))
            .await
            .unwrap()
        else {
            panic!("fork expected");
        };
        let fork = client.read_conversation(branch.id, None, 50).await.unwrap();
        let fork_references: Vec<_> = fork
            .page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::Reference(reference) => Some(reference.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(fork_references, references);
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::Conversation(restored) = client
            .execute(client.prepare(Command::ReadConversation {
                session: session.id,
                before: None,
                limit: 50,
            }))
            .await
            .unwrap()
        else {
            panic!("history expected");
        };
        assert_eq!(restored.page.entries, page.entries);
        assert_eq!(parent.requests.lock().unwrap().len(), 1);
        assert_eq!(child.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn accepts_role_without_text() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
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
        let parent = Server::start(false).await;
        let child = Server::start(false).await;
        let (session, _, role) = setup(&client, &root, &parent, &child).await;
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: Input {
                    references: vec![Reference {
                        target: Target::Agent(role.reference()),
                        label: "@review".into(),
                    }],
                    ..Default::default()
                },
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected");
        };
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(page.children.len(), 1);
        assert_eq!(page.children[0].run.status, Status::Completed);
        assert_eq!(parent.requests.lock().unwrap().len(), 1);
        assert_eq!(child.requests.lock().unwrap().len(), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
