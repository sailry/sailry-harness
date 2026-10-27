use super::*;
use sailry_link::CancellationToken;
use serde_json::json;

async fn held(fixture: &Fixture, message: Input) -> QueuedTurn {
    let Output::QueuedTurn(turn) = fixture
        .client
        .execute(fixture.client.prepare(Command::QueueTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message,
        }))
        .await
        .unwrap()
    else {
        panic!("queued turn expected")
    };
    turn
}

async fn attachment(fixture: &Fixture, bytes: &[u8]) -> attachment::Attachment {
    let Output::AttachmentUpload(upload) = fixture
        .client
        .execute(
            fixture
                .client
                .prepare(Command::UploadAttachment(attachment::Spec {
                    worktree: fixture.session.worktree,
                    name: "context.txt".into(),
                    media_type: "text/plain".into(),
                    size: bytes.len() as u64,
                    revision: blake3::hash(bytes).to_hex().to_string(),
                })),
        )
        .await
        .unwrap()
    else {
        panic!("upload expected")
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
    attachment
}

fn guide(fixture: &Fixture, turn: TurnId, recipient: TurnId, revision: u64) -> Request {
    fixture.client.prepare(Command::GuideQueuedTurn {
        turn,
        recipient,
        expected_revision: revision,
    })
}

fn status(page: &Page, turn: TurnId) -> Status {
    page.runs
        .iter()
        .find(|run| run.turn == turn)
        .unwrap()
        .status
}

async fn tools(fixture: &mut Fixture) {
    let mut provider = fixture.provider.clone();
    provider.default_model = "fixture-a".into();
    provider.models = vec![Model {
        id: "fixture-a".into(),
        display_name: None,
        context: 128_000,
        output: 2048,
        vision: false,
        tools: true,
        reasoning: false,
        web_search: false,
        generates: Vec::new(),
        efforts: Vec::new(),
        capabilities: None,
        custom_efforts: false,
        compaction_tokens: None,
        default_effort: Effort::Default,
    }];
    let Output::Provider(provider) = fixture
        .client
        .execute(fixture.client.prepare(Command::PutProvider {
            provider,
            expected_revision: fixture.provider.revision,
        }))
        .await
        .unwrap()
    else {
        panic!("provider expected")
    };
    fixture.provider = provider;
}

/// Starts a task waiting on a user answer before its next model boundary.
async fn asking(remote: bool) -> (Fixture, QueuedTurn) {
    let server = Server::tools(vec![(
        "ask_user".into(),
        json!({"prompt": "Describe the task", "input": {"kind": "text", "multiline": true, "max_bytes": 1024}}),
    )])
    .await;
    let mut fixture = Fixture::with_server(remote, server).await;
    tools(&mut fixture).await;
    let active = fixture.push("Initial task").await;
    questions::pending(&fixture.client, fixture.session.id).await;
    (fixture, active)
}

#[tokio::test]
async fn settles_tool_batch() {
    for remote in [false, true] {
        let server = Server::batch(vec![
            (
                plugin_tool("files", "write_file"),
                json!({"path":"first.txt","text":"settled result","expected_revision":null}),
            ),
            (
                plugin_tool("files", "write_file"),
                json!({"path":"second.txt","text":"denied result","expected_revision":null}),
            ),
        ])
        .await;
        let mut fixture = Fixture::with_server(remote, server).await;
        tools(&mut fixture).await;
        let active = fixture.push("Perform the requested writes").await;
        let (_, first) = approvals::pending(&fixture.client, fixture.session.id).await;
        let text = "Use the settled evidence and change direction 中文 🙂";
        let selected = held(&fixture, text.into()).await;
        let request = guide(&fixture, selected.id, active.id, 1);
        let admission = fixture.client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let accepted = admission.completion.await.unwrap().unwrap();
        let page = history(&fixture.client, fixture.session.id).await;
        assert_eq!(status(&page, active.id), Status::Running);
        assert_eq!(status(&page, selected.id), Status::Queued);
        assert_eq!(page.queue.items[0].guidance, Some(active.id));
        assert!(page.queue.items[0].ready);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: fixture.session.id,
                approval: first.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let (page, second) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_ne!(first.id, second.id);
        assert_eq!(status(&page, active.id), Status::Running);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveApproval {
                session: fixture.session.id,
                approval: second.id,
                decision: Decision::Deny,
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, active.id), Status::Completed);
        assert_eq!(status(&page, selected.id), Status::Continued);
        assert!(!page.queue.paused);
        assert!(page.queue.items.is_empty());
        let results: Vec<_> = page
            .entries
            .iter()
            .filter(|entry| entry.turn == active.id)
            .flat_map(|entry| &entry.parts)
            .filter(|part| matches!(part, Part::ToolResult { .. }))
            .collect();
        assert_eq!(results.len(), 2);
        assert!(
            !page
                .entries
                .iter()
                .filter(|entry| entry.turn == selected.id && entry.author != "user")
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(part, Part::Text(_)))
        );
        assert_eq!(
            page.entries
                .iter()
                .filter(|entry| entry.turn == selected.id && entry.author == "user")
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::Text(value) if value == text))
                .count(),
            1
        );
        let requests = fixture.server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(prompt(&requests[1]), text);
        assert_eq!(
            requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["role"] == "tool")
                .count(),
            2
        );
        let root = fixture.directory.path().join("project");
        assert_eq!(
            std::fs::read_to_string(root.join("first.txt")).unwrap(),
            "settled result"
        );
        assert!(!root.join("second.txt").exists());
        assert_eq!(fixture.client.execute(request).await.unwrap(), accepted);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        fixture.close().await;
    }
}

#[tokio::test]
async fn waits_for_the_awaited_response() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let server = Server::markdown_held("Natural answer".into(), release.clone()).await;
        let fixture = Fixture::with_server(remote, server).await;
        let active = fixture.push("Initial task").await;
        fixture.server.wait_count(1).await;
        let selected = held(&fixture, "Change direction now".into()).await;
        fixture
            .client
            .execute(guide(&fixture, selected.id, active.id, 1))
            .await
            .unwrap();
        let page = history(&fixture.client, fixture.session.id).await;
        assert_eq!(status(&page, active.id), Status::Running);
        assert_eq!(status(&page, selected.id), Status::Queued);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        release.cancel();
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, active.id), Status::Completed);
        assert_eq!(status(&page, selected.id), Status::Continued);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        assert!(page.queue.items.is_empty());
        fixture.close().await;
    }
}

#[tokio::test]
async fn rejects_unsupported_guidance_attachments() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let server = Server::markdown_held("Natural answer".into(), release.clone()).await;
        let mut fixture = Fixture::with_server(remote, server).await;
        tools(&mut fixture).await;
        let mut provider = fixture.provider.clone();
        provider.models[0].tools = false;
        provider.models[0].capabilities = Some(discovery::Capabilities {
            vision: Some(false),
            tools: Some(false),
            ..Default::default()
        });
        let Output::Provider(provider) = fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                provider,
                expected_revision: fixture.provider.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("provider expected")
        };
        assert!(!provider.models[0].tools);
        assert!(!provider.models[0].vision);
        fixture.provider = provider;
        let active = fixture.push("Initial task").await;
        fixture.server.wait_count(1).await;
        let file = attachment(&fixture, b"Unsupported binary\0\xff").await;
        let selected = held(
            &fixture,
            Input {
                text: "Use this file".into(),
                attachments: vec![file.id],
                references: vec![],
            },
        )
        .await;
        fixture
            .client
            .execute(guide(&fixture, selected.id, active.id, 1))
            .await
            .unwrap();
        release.cancel();
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, active.id), Status::Failed);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        assert!(
            page.entries
                .iter()
                .any(|entry| entry.turn == selected.id && entry.author == "user")
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn keeps_turn_through_repeated_guidance() {
    for remote in [false, true] {
        let question_args = |prompt: &str| json!({"prompt":prompt,"input":{"kind":"text","multiline":true,"max_bytes":1024}});
        let server = Server::tools(vec![
            ("ask_user".into(), question_args("First question")),
            ("ask_user".into(), question_args("Second question")),
        ])
        .await;
        let mut fixture = Fixture::with_server(remote, server).await;
        tools(&mut fixture).await;
        let active = fixture.push("Initial task").await;
        let (_, question) = questions::pending(&fixture.client, fixture.session.id).await;
        std::fs::write(
            fixture.directory.path().join("node/instructions.md"),
            "Fresh preference at the guidance boundary",
        )
        .unwrap();
        let first = held(&fixture, "Use the first detail".into()).await;
        fixture
            .client
            .execute(guide(&fixture, first.id, active.id, 1))
            .await
            .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveQuestion {
                session: fixture.session.id,
                question: question.id,
                response: question::Response::Answer(question::Answer::Text("Continue".into())),
            }))
            .await
            .unwrap();
        let (_, question) = questions::pending(&fixture.client, fixture.session.id).await;
        let later = held(&fixture, "Also use the later detail".into()).await;
        fixture
            .client
            .execute(guide(&fixture, later.id, active.id, 1))
            .await
            .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveQuestion {
                session: fixture.session.id,
                question: question.id,
                response: question::Response::Answer(question::Answer::Text("Continue".into())),
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, active.id), Status::Completed);
        assert!(page.queue.items.is_empty());
        for input in [&active, &first, &later] {
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| entry.turn == input.id && entry.author == "user")
                    .count(),
                1
            );
        }
        assert!(
            page.entries
                .iter()
                .filter(|entry| entry.author == "assistant")
                .all(|entry| entry.turn == active.id)
        );
        let requests = fixture.server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        assert!(
            !requests[0]["messages"]
                .to_string()
                .contains("Fresh preference at the guidance boundary")
        );
        for request in &requests[1..] {
            assert_eq!(
                request["messages"]
                    .to_string()
                    .matches("Fresh preference at the guidance boundary")
                    .count(),
                1
            );
        }
        for pair in requests.windows(2) {
            let prefix = pair[0]["messages"].as_array().unwrap();
            assert_eq!(
                serde_json::to_vec(prefix).unwrap(),
                serde_json::to_vec(&pair[1]["messages"].as_array().unwrap()[..prefix.len()])
                    .unwrap()
            );
            assert_eq!(pair[0]["tools"], pair[1]["tools"]);
        }
        for text in ["Use the first detail", "Also use the later detail"] {
            assert_eq!(
                requests[2]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|message| message["role"] == "user"
                        && message["content"]
                            .as_str()
                            .is_some_and(|content| content.ends_with(text)))
                    .count(),
                1
            );
        }
        fixture.close().await;
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[tokio::test]
async fn retains_recipient_hooks() {
    use sailry_protocol::hooks::{Script, Settings};

    for remote in [false, true] {
        for enabled in [false, true] {
            let release = CancellationToken::new();
            let server = Server::markdown_held("Natural answer".into(), release.clone()).await;
            let fixture = Fixture::with_server(remote, server).await;
            let Output::HookSettings(initial) = fixture
                .client
                .execute(fixture.client.prepare(Command::SaveHooks {
                    worktree: fixture.session.worktree,
                    expected_revision: 0,
                    settings: Settings {
                        enabled,
                        turn_end: Some(Script {
                            command: "printf captured > guidance-hooks.txt".into(),
                            timeout_ms: 5_000,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                }))
                .await
                .unwrap()
            else {
                panic!("hook settings expected")
            };
            let active = fixture.push("Initial task").await;
            fixture.server.wait_count(1).await;
            fixture
                .client
                .execute(fixture.client.prepare(Command::SaveHooks {
                    worktree: fixture.session.worktree,
                    expected_revision: initial.settings.revision,
                    settings: Settings {
                        enabled: !enabled,
                        turn_end: Some(Script {
                            command: "printf changed > guidance-hooks.txt".into(),
                            timeout_ms: 5_000,
                            ..Default::default()
                        }),
                        ..initial.settings
                    },
                }))
                .await
                .unwrap();
            let source = held(&fixture, "Change direction now".into()).await;
            fixture.pause(true).await;
            let Output::Queue(selected) = fixture
                .client
                .execute(guide(&fixture, source.id, active.id, 1))
                .await
                .unwrap()
            else {
                panic!("queue expected")
            };
            assert!(!selected.queue.paused);
            let selected = selected.queue.items[0].turn;
            assert_ne!(selected, source.id);
            assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
            release.cancel();
            fixture.server.wait_count(2).await;
            let page = finished(&fixture.client, fixture.session.id, active.id).await;
            assert_eq!(status(&page, active.id), Status::Completed);
            assert_eq!(status(&page, source.id), Status::Cancelled);
            assert_eq!(status(&page, selected), Status::Continued);
            let output = fixture.directory.path().join("project/guidance-hooks.txt");
            if enabled {
                assert_eq!(std::fs::read_to_string(output).unwrap(), "captured");
            } else {
                assert!(!output.exists());
            }
            assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
            fixture.close().await;
        }
    }
}

#[tokio::test]
async fn retains_streamed_text() {
    for remote in [false, true] {
        let gates: Vec<_> = (0..3)
            .map(|_| std::sync::Arc::new(tokio::sync::Notify::new()))
            .collect();
        let server = Server::staged(gates.clone()).await;
        let fixture = Fixture::with_server(remote, server).await;
        let active = fixture.push("Initial task").await;
        let mut stream = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let streamed = |draft: &sailry_protocol::conversation::Draft| {
            draft.turn == active.id && draft.parts == vec![Part::Text("First".into())]
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match stream.next().await.unwrap() {
                    Update::ConversationSnapshot(snapshot)
                        if snapshot.drafts.iter().any(streamed) =>
                    {
                        break;
                    }
                    Update::ConversationFrame(frame)
                        if matches!(&frame.change, sailry_protocol::conversation::Change::Delta(draft) if streamed(draft)) =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        let selected = held(&fixture, "Stop and summarize".into()).await;
        fixture
            .client
            .execute(guide(&fixture, selected.id, active.id, 1))
            .await
            .unwrap();
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        for gate in &gates {
            gate.notify_one();
        }
        fixture.server.wait_count(2).await;
        let page = history(&fixture.client, fixture.session.id).await;
        assert_eq!(status(&page, active.id), Status::Running);
        assert_eq!(
            page.entries
                .iter()
                .filter(|entry| entry.turn == active.id
                    && entry.parts == vec![Part::Text("First second third".into())])
                .count(),
            1
        );
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StopTurn { turn: active.id }),
            )
            .await
            .unwrap();
        finished(&fixture.client, fixture.session.id, active.id).await;
        fixture.close().await;
    }
}

#[tokio::test]
async fn preserves_stopped_input() {
    for remote in [false, true] {
        let (mut fixture, active) = asking(remote).await;
        let attachment = attachment(&fixture, b"Retained attachment").await;
        let input = Input {
            text: "Retain this guidance 中文 🙂\n".repeat(100),
            attachments: vec![attachment.id],
            references: vec![reference::Reference {
                label: "context.txt".into(),
                target: reference::Target::File("context.txt".into()),
            }],
        };
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config: SessionConfig {
                    permission: Permission::Full,
                    ..fixture.session.config.clone()
                },
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let original = held(
            &fixture,
            Input {
                text: "Replace this draft".into(),
                ..input.clone()
            },
        )
        .await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::EditQueuedTurn {
                turn: original.id,
                expected_revision: 1,
                message: input.clone(),
            }))
            .await
            .unwrap();
        fixture
            .client
            .execute(guide(&fixture, original.id, active.id, 2))
            .await
            .unwrap();
        let queue = fixture.queue().await;
        let replacement = queue
            .items
            .iter()
            .find(|item| item.guidance == Some(active.id))
            .unwrap();
        assert_ne!(replacement.turn, original.id);
        let selected = read(&fixture.client, replacement.turn).await.turn;
        assert_eq!(selected.config, active.config);
        assert_eq!(selected.roles, active.roles);
        assert_eq!(selected.plugins, active.plugins);
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StopTurn { turn: active.id }),
            )
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, active.id), Status::Cancelled);
        assert_eq!(status(&page, original.id), Status::Cancelled);
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == original.id)
                .unwrap()
                .revision,
            original.revision
        );
        assert_eq!(status(&page, selected.id), Status::Queued);
        assert!(page.queue.paused);
        assert_eq!(page.queue.items[0].guidance, None);
        assert!(page.queue.items[0].truncated);
        assert_eq!(page.queue.items[0].attachments[0].id, attachment.id);
        assert_eq!(read(&fixture.client, selected.id).await.message, input);
        let fixture = fixture.restart().await;
        let page = history(&fixture.client, fixture.session.id).await;
        assert!(page.queue.paused);
        assert_eq!(page.queue.items[0].guidance, None);
        assert_eq!(status(&page, selected.id), Status::Queued);
        assert_eq!(read(&fixture.client, selected.id).await.message, input);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        fixture.close().await;
    }
}

#[tokio::test]
async fn retains_each_selection() {
    for remote in [false, true] {
        let (fixture, active) = asking(remote).await;
        let first = held(&fixture, "First guidance".into()).await;
        let second = held(&fixture, "Second guidance".into()).await;
        fixture
            .client
            .execute(guide(&fixture, first.id, active.id, 1))
            .await
            .unwrap();
        fixture
            .client
            .execute(guide(&fixture, second.id, active.id, 1))
            .await
            .unwrap();
        let queue = fixture.queue().await;
        assert_eq!(queue.items[0].turn, first.id);
        assert_eq!(queue.items[0].guidance, Some(active.id));
        assert_eq!(queue.items[0].revision, 2);
        assert_eq!(queue.items[1].turn, second.id);
        assert_eq!(queue.items[1].guidance, Some(active.id));
        assert_eq!(
            read(&fixture.client, first.id).await.message.text,
            "First guidance"
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::WithdrawGuidance {
                turn: second.id,
                expected_revision: queue.items[1].revision,
            }))
            .await
            .unwrap();
        let withdrawn = fixture.queue().await;
        assert_eq!(withdrawn.items[0], queue.items[0]);
        assert_eq!(withdrawn.items[1].turn, second.id);
        assert_eq!(withdrawn.items[1].guidance, None);
        assert!(!withdrawn.items[1].ready);
        assert_eq!(
            read(&fixture.client, second.id).await.message.text,
            "Second guidance"
        );
        fixture
            .client
            .execute(fixture.client.prepare(Command::EditQueuedTurn {
                turn: second.id,
                expected_revision: withdrawn.items[1].revision,
                message: "Edited guidance".into(),
            }))
            .await
            .unwrap();
        let edited = fixture.queue().await;
        assert_eq!(edited.items[0], queue.items[0]);
        assert_eq!(edited.items[1].guidance, None);
        assert_eq!(
            fixture
                .client
                .execute(guide(
                    &fixture,
                    second.id,
                    active.id,
                    queue.items[1].revision
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            status(
                &history(&fixture.client, fixture.session.id).await,
                active.id
            ),
            Status::Running
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        let (_, question) = questions::pending(&fixture.client, fixture.session.id).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveQuestion {
                session: fixture.session.id,
                question: question.id,
                response: question::Response::Answer(question::Answer::Text("Continue".into())),
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, second.id), Status::Queued);
        assert_eq!(page.queue.items.len(), 1);
        assert_eq!(page.queue.items[0].turn, second.id);
        assert!(!page.queue.items[0].ready);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        fixture.close().await;
    }
}

#[tokio::test]
async fn batches_inputs_with_one_output_owner() {
    for remote in [false, true] {
        let server = Server::scripted_tools(vec![(
            "ask_user".into(),
            json!({"prompt":"Describe the task","input":{"kind":"text","multiline":true,"max_bytes":1024}}),
        )]).await;
        let mut fixture = Fixture::with_server(remote, server).await;
        tools(&mut fixture).await;
        std::fs::write(
            fixture.directory.path().join("project/evidence.txt"),
            "Captured batch evidence",
        )
        .unwrap();
        let active = fixture.push("Initial task").await;
        let (_, question) = questions::pending(&fixture.client, fixture.session.id).await;
        std::fs::create_dir(fixture.directory.path().join("project/nested")).unwrap();
        std::fs::write(
            fixture.directory.path().join("project/nested/AGENTS.md"),
            "Guidance folder instructions",
        )
        .unwrap();
        std::fs::write(
            fixture.directory.path().join("project/nested/evidence.txt"),
            "Nested evidence",
        )
        .unwrap();
        let attachment = attachment(&fixture, b"Batch attachment evidence").await;
        let first = held(&fixture, "Read the evidence".into()).await;
        let second = held(
            &fixture,
            Input {
                text: "Report both instructions 中文 🙂".into(),
                attachments: vec![attachment.id],
                references: vec![
                    reference::Reference {
                        target: reference::Target::Plugin("files".into()),
                        label: "Files".into(),
                    },
                    reference::Reference {
                        target: reference::Target::File("nested/evidence.txt".into()),
                        label: "Nested evidence".into(),
                    },
                ],
            },
        )
        .await;
        let first_request = guide(&fixture, first.id, active.id, 1);
        let first_result = fixture.client.execute(first_request.clone()).await.unwrap();
        fixture
            .client
            .execute(guide(&fixture, second.id, active.id, 1))
            .await
            .unwrap();
        let pending = history(&fixture.client, fixture.session.id).await;
        assert_eq!(
            sailry_client::conversation::turns::guided(&pending)
                .iter()
                .map(|run| run.turn)
                .collect::<Vec<_>>(),
            [first.id, second.id]
        );
        for turn in [&first, &second] {
            let run = pending.runs.iter().find(|run| run.turn == turn.id).unwrap();
            assert_eq!(run.revision, turn.revision);
            assert_eq!(
                read(&fixture.client, turn.id).await.turn.request,
                turn.request
            );
        }
        fixture.server.replace_tools(vec![(
            plugin_tool("files", "read_file"),
            json!({"path":"evidence.txt"}),
        )]);
        fixture
            .client
            .execute(fixture.client.prepare(Command::ResolveQuestion {
                session: fixture.session.id,
                question: question.id,
                response: question::Response::Answer(question::Answer::Text("Continue".into())),
            }))
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, active.id), Status::Completed);
        assert_eq!(status(&page, first.id), Status::Continued);
        let joined = page.runs.iter().find(|run| run.turn == second.id).unwrap();
        assert_eq!(joined.status, Status::Continued);
        assert_eq!(joined.joined, Some(active.id));
        assert!(page.queue.items.is_empty());
        let entries: Vec<_> = page
            .entries
            .iter()
            .filter(|entry| entry.author == "user" && [first.id, second.id].contains(&entry.turn))
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!([entries[0].turn, entries[1].turn], [first.id, second.id]);
        assert!(
            entries[1]
                .parts
                .iter()
                .any(|part| matches!(part, Part::Attachment(value) if value.id == attachment.id))
        );
        assert!(sailry_client::conversation::turns::guided(&page).is_empty());
        assert!(
            !page
                .entries
                .iter()
                .any(|entry| entry.turn == second.id && entry.author != "user")
        );
        let view = progress::observe(&fixture.client, fixture.session.id).await;
        let call = view
            .calls
            .iter()
            .find(|call| call.name == plugin_tool("files", "read_file"))
            .unwrap();
        assert_eq!(call.turn, active.id);
        let reference = call.detail_reference(&page).unwrap();
        assert_eq!(reference.turn, active.id);
        let detail = fixture.client.tool_detail(reference).await.unwrap();
        assert!(
            detail
                .result
                .as_ref()
                .unwrap()
                .to_string()
                .contains("Captured batch evidence")
        );
        let requests = fixture.server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        let inputs: Vec<_> = requests[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .collect();
        let inputs = &inputs[inputs.len() - 2..];
        assert!(
            inputs[0]["content"]
                .to_string()
                .contains("Read the evidence")
        );
        assert!(
            inputs[1]["content"]
                .to_string()
                .contains("Report both instructions 中文 🙂")
        );
        assert!(
            inputs[1]["content"]
                .to_string()
                .contains(&attachment.id.to_string())
        );
        assert!(
            requests[1]["messages"]
                .to_string()
                .contains("The user explicitly selected plugin files for this task.")
        );
        assert!(
            requests[1]["messages"]
                .to_string()
                .contains("Guidance folder instructions")
        );
        assert_eq!(requests[0]["messages"][0], requests[1]["messages"][0]);
        assert_eq!(requests[0]["tools"], requests[1]["tools"]);
        assert_eq!(
            fixture.client.execute(first_request).await.unwrap(),
            first_result
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        fixture.close().await;
    }
}

#[tokio::test]
async fn retains_recipient_capture() {
    for remote in [false, true] {
        let (mut fixture, active) = asking(remote).await;
        for config in [
            SessionConfig {
                effort: Effort::Low,
                ..fixture.session.config.clone()
            },
            SessionConfig {
                mode: WorkMode::Plan,
                ..fixture.session.config.clone()
            },
            SessionConfig {
                permission: Permission::Full,
                ..fixture.session.config.clone()
            },
        ] {
            let Output::Session(session) = fixture
                .client
                .execute(fixture.client.prepare(Command::SetSessionConfig {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    config,
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            fixture.session = session;
            let selected = held(&fixture, "Do not change the active capture".into()).await;
            let request = guide(&fixture, selected.id, active.id, 1);
            let accepted = fixture.client.execute(request.clone()).await.unwrap();
            let queue = fixture.queue().await;
            let replacement = queue
                .items
                .iter()
                .find(|item| item.guidance == Some(active.id))
                .unwrap();
            assert_ne!(replacement.turn, selected.id);
            let saved = read(&fixture.client, replacement.turn).await;
            assert_eq!(saved.turn.config, active.config);
            assert_eq!(saved.turn.plugins, active.plugins);
            assert_eq!(saved.turn.roles, active.roles);
            assert_eq!(saved.message.text, "Do not change the active capture");
            let page = history(&fixture.client, fixture.session.id).await;
            let original = page
                .runs
                .iter()
                .find(|run| run.turn == selected.id)
                .unwrap();
            assert_eq!(original.status, Status::Cancelled);
            assert_eq!(original.revision, selected.revision);
            assert_eq!(status(&page, active.id), Status::Running);
            assert_eq!(fixture.client.execute(request).await.unwrap(), accepted);
            assert_eq!(fixture.queue().await, queue);
        }
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config: active.config.clone(),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let Output::Plugin(files) = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadPlugin {
                name: "files".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("plugin expected")
        };
        fixture
            .client
            .execute(fixture.client.prepare(Command::SetPluginEnabled {
                name: files.summary.name,
                expected_revision: files.summary.revision,
                enabled: false,
            }))
            .await
            .unwrap();
        let selected = held(&fixture, "Do not replace captured tools".into()).await;
        let request = guide(&fixture, selected.id, active.id, 1);
        let accepted = fixture.client.execute(request.clone()).await.unwrap();
        let queue = fixture.queue().await;
        let replacement = queue
            .items
            .iter()
            .find(|item| {
                item.guidance == Some(active.id) && item.preview == "Do not replace captured tools"
            })
            .unwrap();
        assert_ne!(replacement.turn, selected.id);
        let saved = read(&fixture.client, replacement.turn).await;
        assert_eq!(saved.turn.config, active.config);
        assert_eq!(saved.turn.plugins, active.plugins);
        assert_eq!(saved.turn.roles, active.roles);
        assert_eq!(saved.message.text, "Do not replace captured tools");
        let original = fixture
            .client
            .read_turn_capture(fixture.session.id, selected.id)
            .await
            .unwrap();
        assert_ne!(original.plugins, active.plugins);
        let page = history(&fixture.client, fixture.session.id).await;
        assert_eq!(status(&page, selected.id), Status::Cancelled);
        assert_eq!(status(&page, active.id), Status::Running);
        assert_eq!(fixture.client.execute(request).await.unwrap(), accepted);
        assert_eq!(fixture.queue().await, queue);

        let Output::Session(other) = fixture
            .client
            .execute(fixture.client.prepare(Command::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(active.config.clone()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(other_active) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: other.id,
                expected_revision: other.revision,
                message: "Other captured task".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        fixture.server.wait_count(2).await;
        let quick = Server::start(false).await;
        let mut provider = fixture.provider.clone();
        provider.endpoint = quick.endpoint.clone();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                provider,
                expected_revision: fixture.provider.revision,
            }))
            .await
            .unwrap();
        let Output::QueuedTurn(other_source) = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: other.id,
                expected_revision: other.revision,
                message: "Do not replace the captured provider".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        fixture
            .client
            .execute(guide(&fixture, other_source.id, other_active.id, 1))
            .await
            .unwrap();
        let queue = history(&fixture.client, other.id).await.queue;
        let replacement = queue
            .items
            .iter()
            .find(|item| item.guidance == Some(other_active.id))
            .unwrap();
        let saved = read(&fixture.client, replacement.turn).await;
        assert_eq!(saved.turn.config, other_active.config);
        assert_eq!(saved.message.text, "Do not replace the captured provider");
        assert_eq!(
            fixture
                .client
                .execute(guide(&fixture, selected.id, other_active.id, 1))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            status(
                &history(&fixture.client, fixture.session.id).await,
                active.id
            ),
            Status::Running
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        fixture
            .client
            .execute(fixture.client.prepare(Command::SendQueuedTurn {
                turn: replacement.turn,
                expected_revision: replacement.revision,
            }))
            .await
            .unwrap();
        fixture.server.wait_count(3).await;
        assert!(quick.requests.lock().unwrap().is_empty());
        fixture
            .client
            .execute(fixture.client.prepare(Command::StopTurn {
                turn: replacement.turn,
            }))
            .await
            .unwrap();
        fixture.close().await;
    }
}
