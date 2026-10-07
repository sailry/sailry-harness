use super::fixture::Fixture;
use super::*;

fn status(page: &Page, turn: TurnId) -> Status {
    page.runs
        .iter()
        .find(|run| run.turn == turn)
        .unwrap()
        .status
}

async fn active(fixture: &Fixture) -> (Session, Run) {
    let child = children(&fixture.client, fixture.session.id, 1)
        .await
        .pop()
        .unwrap();
    let (_, approval) = crate::approvals::pending(&fixture.client, child.id).await;
    let page = history(&fixture.client, child.id).await;
    let run = page
        .runs
        .iter()
        .find(|run| run.turn == approval.turn)
        .unwrap()
        .clone();
    (child, run)
}

async fn input(fixture: &Fixture, child: &Session) -> Input {
    std::fs::write(
        fixture.root().join("context.txt"),
        "Selected workspace context",
    )
    .unwrap();
    let attachment = fixture
        .upload(child.worktree, b"Selected guidance attachment")
        .await;
    Input {
        text: "Continue using the settled evidence 中文 🙂\n".repeat(20),
        references: vec![reference::Reference {
            target: reference::Target::File("context.txt".into()),
            label: "context.txt".into(),
        }],
        attachments: vec![attachment.id],
    }
}

fn prepare(fixture: &Fixture, recipient: &Run, message: Input) -> Request {
    fixture.client.prepare(Command::GuideTurn {
        recipient: recipient.turn,
        expected_revision: recipient.revision,
        message,
    })
}

async fn read(fixture: &Fixture, turn: TurnId) -> QueuedMessage {
    let Output::QueuedMessage(message) = fixture
        .client
        .execute(fixture.client.prepare(Command::ReadQueuedTurn { turn }))
        .await
        .unwrap()
    else {
        panic!("queued message expected")
    };
    message
}

async fn batch(remote: bool) -> Fixture {
    let parent = Server::turn_tools(vec![(
        plugin_tool("delegation", "spawn_agent"),
        json!({"role":"review","task":"Perform the requested writes"}),
    )])
    .await;
    let child = Server::batch(vec![
        (
            plugin_tool("files", "write_file"),
            json!({"path":"first.txt","text":"settled evidence","expected_revision":null}),
        ),
        (
            plugin_tool("files", "write_file"),
            json!({"path":"second.txt","text":"denied evidence","expected_revision":null}),
        ),
    ])
    .await;
    Fixture::start(remote, parent, child).await
}

#[tokio::test]
async fn settles_batch_before_handoff() {
    for remote in [false, true] {
        let mut fixture = batch(remote).await;
        let parent = fixture.push("Parent-only private context").await;
        let (child, recipient) = active(&fixture).await;
        let (_, first) = crate::approvals::pending(&fixture.client, child.id).await;
        let message = input(&fixture, &child).await;
        let request = prepare(&fixture, &recipient, message.clone());
        let admission = fixture.client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let accepted = admission.completion.await.unwrap().unwrap();
        let Output::QueuedTurn(guided) = &accepted else {
            panic!("guided turn expected")
        };
        let guided = guided.clone();
        assert_eq!(guided.session, child.id);
        assert_eq!(guided.config, child.config);
        assert_eq!(guided.revision, recipient.revision);
        assert_eq!(
            fixture.client.execute(request.clone()).await.unwrap(),
            accepted
        );
        assert_eq!(read(&fixture, guided.id).await.message, message);
        assert_eq!(
            fixture
                .client
                .execute(prepare(&fixture, &recipient, "Another direction".into()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        let page = history(&fixture.client, child.id).await;
        assert_eq!(status(&page, recipient.turn), Status::Running);
        assert_eq!(status(&page, guided.id), Status::Queued);
        assert_eq!(page.queue.items[0].guidance, Some(recipient.turn));
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        fixture.provider.endpoint = "http://127.0.0.1:1/v1".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                provider: fixture.provider.clone(),
                expected_revision: fixture.provider.revision,
            }))
            .await
            .unwrap();
        let mut changed = fixture.role.clone();
        changed.instructions = "Changed catalog instructions".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutRole {
                role: changed,
                expected_revision: fixture.role.revision,
            }))
            .await
            .unwrap();
        disable_tools(&fixture.client, &["files"]).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: child.id,
                approval: first.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let (page, second) = crate::approvals::pending(&fixture.client, child.id).await;
        assert_ne!(first.id, second.id);
        assert_eq!(status(&page, recipient.turn), Status::Running);
        assert_eq!(status(&page, guided.id), Status::Queued);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: child.id,
                approval: second.id,
                decision: Decision::Deny,
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, child.id, guided.id).await;
        assert_eq!(status(&page, recipient.turn), Status::Continued);
        assert_eq!(status(&page, guided.id), Status::Completed);
        assert!(page.queue.items.is_empty());
        assert_eq!(
            page.entries
                .iter()
                .filter(|entry| entry.turn == recipient.turn)
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::ToolResult { .. }))
                .count(),
            2
        );
        assert!(page.entries.iter().any(|entry| entry.turn == guided.id
            && entry.parts.contains(&Part::Text(message.text.clone()))));
        assert!(page.entries.iter().any(|entry| {
            entry.turn == guided.id
                && entry
                    .parts
                    .contains(&Part::Reference(message.references[0].clone()))
        }));
        let parent_page = finished(&fixture.client, fixture.session.id, parent.id).await;
        assert_eq!(status(&parent_page, parent.id), Status::Completed);
        assert_eq!(parent_page.children.len(), 2);
        assert_eq!(
            parent_page.children[0].origin,
            parent_page.children[1].origin
        );
        assert_eq!(parent_page.children[0].run.turn, recipient.turn);
        assert_eq!(parent_page.children[1].run.turn, guided.id);
        let returned = results(&parent_page);
        assert_eq!(returned.len(), 1);
        assert_eq!(returned[0]["session"], json!(child.id));
        assert_eq!(returned[0]["turn"], json!(guided.id));
        assert_eq!(returned[0]["status"], "completed");
        let requests = fixture.child.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["model"], "fixture-child");
        let context = requests[1]["messages"].to_string();
        assert!(context.contains("settled evidence"));
        assert!(context.contains("Selected guidance attachment"));
        assert!(context.contains("Frozen review instructions"));
        assert!(!context.contains("Changed catalog instructions"));
        assert!(!context.contains("Parent-only private context"));
        assert!(
            requests[1]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| { tool["function"]["name"] == plugin_tool("files", "read_file") })
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root().join("first.txt")).unwrap(),
            "settled evidence"
        );
        assert!(!fixture.root().join("second.txt").exists());
        assert_eq!(fixture.client.execute(request).await.unwrap(), accepted);
        assert_eq!(
            children(&fixture.client, fixture.session.id, 1).await.len(),
            1
        );
        let fixture = fixture.restart().await;
        assert_eq!(
            history(&fixture.client, child.id).await.entries,
            page.entries
        );
        assert_eq!(
            history(&fixture.client, fixture.session.id).await.children,
            parent_page.children
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 2);
        fixture.close().await;
    }
}

#[tokio::test]
async fn preserves_stopped_and_interrupted_input() {
    for remote in [false, true] {
        for target in [Some(true), Some(false), None] {
            let fixture = batch(remote).await;
            let parent = fixture.push("Parent request").await;
            let (child, recipient) = active(&fixture).await;
            let message = input(&fixture, &child).await;
            let Output::QueuedTurn(guided) = fixture
                .client
                .execute(prepare(&fixture, &recipient, message.clone()))
                .await
                .unwrap()
            else {
                panic!("guided turn expected")
            };
            if let Some(parent_target) = target {
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::StopTurn {
                        turn: if parent_target {
                            parent.id
                        } else {
                            recipient.turn
                        },
                    }))
                    .await
                    .unwrap();
                let page = finished(&fixture.client, child.id, recipient.turn).await;
                assert_eq!(status(&page, recipient.turn), Status::Cancelled);
                assert_eq!(status(&page, guided.id), Status::Queued);
                assert!(page.queue.paused);
                assert_eq!(read(&fixture, guided.id).await.message, message);
            }
            let fixture = fixture.restart().await;
            let page = history(&fixture.client, child.id).await;
            assert_eq!(
                status(&page, recipient.turn),
                if target.is_some() {
                    Status::Cancelled
                } else {
                    Status::Interrupted
                }
            );
            assert_eq!(status(&page, guided.id), Status::Queued);
            assert!(page.queue.paused);
            assert_eq!(page.queue.items.len(), 1);
            assert_eq!(page.queue.items[0].guidance, None);
            assert!(page.queue.items[0].truncated);
            assert_eq!(
                page.queue.items[0].attachments[0].id,
                message.attachments[0]
            );
            let saved = read(&fixture, guided.id).await;
            assert_eq!(saved.message, message);
            let Output::AttachmentDownload(download) = fixture
                .client
                .execute(fixture.client.prepare(Command::DownloadAttachment {
                    worktree: child.worktree,
                    attachment: message.attachments[0],
                }))
                .await
                .unwrap()
            else {
                panic!("attachment download expected")
            };
            let mut bytes = Vec::new();
            fixture
                .client
                .download_attachment(
                    &download,
                    &mut bytes,
                    sailry_link::CancellationToken::new(),
                    |_| {},
                )
                .await
                .unwrap();
            assert_eq!(bytes, b"Selected guidance attachment");
            assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
            for command in [
                Command::SubmitTurn {
                    session: child.id,
                    expected_revision: child.revision,
                    message: "Unrelated input".into(),
                },
                Command::StartQueuedTurn { turn: guided.id },
                Command::SendQueuedTurn {
                    turn: guided.id,
                    expected_revision: saved.revision,
                },
                Command::EditQueuedTurn {
                    turn: guided.id,
                    expected_revision: saved.revision,
                    message: "Changed input".into(),
                },
                Command::SetQueuePaused {
                    session: child.id,
                    expected_revision: page.queue.revision,
                    paused: false,
                },
            ] {
                assert_eq!(
                    fixture
                        .client
                        .execute(fixture.client.prepare(command))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::PermissionDenied
                );
            }
            fixture
                .client
                .execute(fixture.client.prepare(Command::RemoveQueuedTurn {
                    turn: guided.id,
                    expected_revision: saved.revision,
                }))
                .await
                .unwrap();
            let page = history(&fixture.client, child.id).await;
            assert!(page.queue.items.is_empty());
            assert_eq!(status(&page, guided.id), Status::Cancelled);
            assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
            fixture.close().await;
        }
    }
}

#[tokio::test]
async fn follows_natural_child_completion() {
    for remote in [false, true] {
        let release = sailry_link::CancellationToken::new();
        let parent = Server::turn_tools(vec![(
            plugin_tool("delegation", "spawn_agent"),
            json!({"role":"review","task":"Initial child task"}),
        )])
        .await;
        let child = Server::markdown_held("Natural child answer".into(), release.clone()).await;
        let fixture = Fixture::start(remote, parent, child).await;
        let parent = fixture.push("Parent request").await;
        fixture.child.wait_count(1).await;
        let child = children(&fixture.client, fixture.session.id, 1)
            .await
            .pop()
            .unwrap();
        let recipient = history(&fixture.client, child.id).await.runs[0].clone();
        let Output::QueuedTurn(guided) = fixture
            .client
            .execute(prepare(
                &fixture,
                &recipient,
                "Follow up after completion".into(),
            ))
            .await
            .unwrap()
        else {
            panic!("guided turn expected")
        };
        release.cancel();
        let page = finished(&fixture.client, child.id, guided.id).await;
        assert_eq!(status(&page, recipient.turn), Status::Completed);
        assert_eq!(status(&page, guided.id), Status::Completed);
        let page = finished(&fixture.client, fixture.session.id, parent.id).await;
        assert_eq!(results(&page)[0]["turn"], json!(guided.id));
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 2);
        fixture.close().await;
    }
}
