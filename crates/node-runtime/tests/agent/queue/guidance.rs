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
async fn retains_unreadable_guidance_attachments() {
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
        let bytes = b"Unsupported binary\0\xff";
        let file = attachment(&fixture, bytes).await;
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
        assert_eq!(status(&page, active.id), Status::Completed);
        assert_eq!(status(&page, selected.id), Status::Continued);
        let requests = fixture.server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        let projected = requests[1]["messages"].to_string();
        assert!(projected.contains(&file.id.to_string()));
        assert!(projected.contains("Attachment reference:"));
        assert!(projected.contains("Contents not included; use read_office"));
        assert!(!projected.contains("Unsupported binary"));
        assert!(
            page.entries
                .iter()
                .any(|entry| entry.turn == selected.id && entry.author == "user")
        );
        let Output::AttachmentDownload(download) = fixture
            .client
            .execute(fixture.client.prepare(Command::DownloadAttachment {
                worktree: fixture.session.worktree,
                attachment: file.id,
            }))
            .await
            .unwrap()
        else {
            panic!("attachment download expected")
        };
        assert_eq!(download.attachment, file);
        let mut original = Vec::new();
        fixture
            .client
            .download_attachment(&download, &mut original, CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert_eq!(original.as_slice(), bytes.as_slice());
        fixture.close().await;
    }
}

#[tokio::test]
async fn retains_input_after_preparation_failure() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let server = Server::markdown_held("Natural answer".into(), release.clone()).await;
        let mut fixture = Fixture::with_server(remote, server).await;
        tools(&mut fixture).await;
        let nested = fixture.directory.path().join("project/nested");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("context.txt"), "Referenced context").unwrap();
        let active = fixture.push("Initial task").await;
        fixture.server.wait_count(1).await;
        let input = Input {
            text: "Keep this guidance 中文 🙂".into(),
            attachments: vec![],
            references: vec![reference::Reference {
                label: "context.txt".into(),
                target: reference::Target::File("nested/context.txt".into()),
            }],
        };
        let selected = held(&fixture, input.clone()).await;
        fixture
            .client
            .execute(guide(&fixture, selected.id, active.id, 1))
            .await
            .unwrap();
        // The guide becomes unreadable after admission, at the next model boundary.
        std::fs::write(nested.join("AGENTS.md"), [255]).unwrap();
        release.cancel();
        let page = finished(&fixture.client, fixture.session.id, active.id).await;
        assert_eq!(status(&page, active.id), Status::Failed);
        let error = page
            .runs
            .iter()
            .find(|run| run.turn == active.id)
            .unwrap()
            .error
            .as_ref()
            .unwrap();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(error.message.contains("nested/AGENTS.md"));
        assert_eq!(status(&page, selected.id), Status::Queued);
        assert!(page.queue.paused);
        assert_eq!(page.queue.items.len(), 1);
        assert_eq!(page.queue.items[0].turn, selected.id);
        assert_eq!(page.queue.items[0].guidance, None);
        assert!(!page.queue.items[0].ready);
        assert!(!page.entries.iter().any(|entry| entry.turn == selected.id));
        let retained = read(&fixture.client, selected.id).await;
        assert_eq!(retained.message, input);
        assert_eq!(retained.turn, selected);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        let fixture = fixture.restart().await;
        let restored = history(&fixture.client, fixture.session.id).await;
        assert_eq!(restored, page);
        assert_eq!(read(&fixture.client, selected.id).await, retained);
        std::fs::write(nested.join("AGENTS.md"), "Use the recovered project guide").unwrap();
        let request = fixture
            .client
            .prepare(Command::StartQueuedTurn { turn: selected.id });
        let accepted = fixture.client.execute(request.clone()).await.unwrap();
        fixture.pause(false).await;
        let completed = finished(&fixture.client, fixture.session.id, selected.id).await;
        assert_eq!(status(&completed, active.id), Status::Failed);
        assert_eq!(status(&completed, selected.id), Status::Completed);
        assert!(completed.queue.items.is_empty());
        assert_eq!(
            completed
                .entries
                .iter()
                .filter(|entry| entry.turn == selected.id && entry.author == "user")
                .flat_map(|entry| &entry.parts)
                .filter(|part| matches!(part, Part::Text(text) if text == &input.text))
                .count(),
            1
        );
        assert_eq!(fixture.client.execute(request).await.unwrap(), accepted);
        assert_eq!(
            history(&fixture.client, fixture.session.id).await,
            completed
        );
        let requests = fixture.server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(requests[1]["messages"].to_string().contains(&input.text));
        assert!(
            requests[1]["messages"]
                .to_string()
                .contains("Use the recovered project guide")
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
        for input in [&first, &later] {
            let run = page.runs.iter().find(|run| run.turn == input.id).unwrap();
            assert_eq!(run.status, Status::Continued);
            assert_eq!(run.joined, Some(active.id));
        }
        for (input, text) in [
            (&active, "Initial task"),
            (&first, "Use the first detail"),
            (&later, "Also use the later detail"),
        ] {
            assert_eq!(
                page.entries
                    .iter()
                    .filter(|entry| entry.turn == input.id
                        && entry.author == "user"
                        && entry
                            .parts
                            .iter()
                            .any(|part| matches!(part, Part::Text(value) if value == text)))
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
        assert!(requests.iter().all(|request| {
            !request["messages"]
                .to_string()
                .contains("Fresh preference at the guidance boundary")
        }));
        let tasks: Vec<_> = requests
            .iter()
            .map(crate::agent_support::task_request)
            .collect();
        for (pair, tasks) in requests.windows(2).zip(tasks.windows(2)) {
            let prefix = tasks[0]["messages"].as_array().unwrap();
            assert_eq!(
                serde_json::to_vec(prefix).unwrap(),
                serde_json::to_vec(&tasks[1]["messages"].as_array().unwrap()[..prefix.len()])
                    .unwrap()
            );
            let context = crate::agent_support::turn_context(&pair[0]).unwrap();
            assert_eq!(crate::agent_support::turn_context(&pair[1]), Some(context));
            // The original wire prefix stays byte-identical up to the
            // latest turn context, including any coalesced user text.
            let encoded = serde_json::to_string(&pair[0]["messages"]).unwrap();
            let tail = serde_json::to_string(&format!("\n{context}")).unwrap();
            let boundary = encoded.rfind(&tail[1..tail.len() - 1]).unwrap();
            let next = serde_json::to_string(&pair[1]["messages"]).unwrap();
            assert!(next.as_bytes().starts_with(&encoded.as_bytes()[..boundary]));
            assert_eq!(pair[0]["tools"], pair[1]["tools"]);
        }
        for text in ["Use the first detail", "Also use the later detail"] {
            assert_eq!(
                tasks[2]["messages"]
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
            assert_eq!(selected.queue.items[0].guidance, None);
            assert!(selected.queue.items[0].ready);
            let selected = selected.queue.items[0].turn;
            assert_eq!(selected, source.id);
            assert_eq!(read(&fixture.client, selected).await.turn, source);
            assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
            release.cancel();
            fixture.server.wait_count(2).await;
            let page = finished(&fixture.client, fixture.session.id, selected).await;
            assert_eq!(status(&page, active.id), Status::Completed);
            assert_eq!(status(&page, source.id), Status::Completed);
            assert!(page.queue.items.is_empty());
            let output = fixture.directory.path().join("project/guidance-hooks.txt");
            if enabled {
                assert_eq!(std::fs::read_to_string(output).unwrap(), "captured");
            } else {
                assert_eq!(std::fs::read_to_string(output).unwrap(), "changed");
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
        let pending = queue
            .items
            .iter()
            .find(|item| item.turn == original.id)
            .unwrap();
        assert_eq!(pending.guidance, None);
        assert!(pending.ready);
        let selected = read(&fixture.client, original.id).await.turn;
        assert_eq!(selected, original);
        assert_eq!(selected.config.permission, Permission::Full);
        assert_eq!(active.config.permission, Permission::Ask);
        assert_eq!(selected.roles, original.roles);
        assert_eq!(selected.plugins, original.plugins);
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
        assert_eq!(status(&page, original.id), Status::Queued);
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
        let retained = read(&fixture.client, selected.id).await;
        assert_eq!(retained.message, input);
        assert_eq!(retained.turn, original);
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
        let detail = crate::agent_support::details::read(&fixture.client, reference)
            .await
            .unwrap();
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
        let input = inputs.last().unwrap()["content"].to_string();
        let first = input.find("Read the evidence").unwrap();
        let second = input.find("Report both instructions 中文 🙂").unwrap();
        assert!(first < second);
        for text in ["Read the evidence", "Report both instructions 中文 🙂"] {
            assert_eq!(input.matches(text).count(), 1);
        }
        assert!(input.contains(&attachment.id.to_string()));
        assert!(input.contains("User-selected references"));
        assert!(input.contains("files"));
        assert!(input.contains("nested/evidence.txt"));
        let capability = "The user explicitly selected plugin files for this task.";
        assert!(
            requests[1]["messages"][0]["content"]
                .to_string()
                .contains(capability)
        );
        assert_eq!(
            requests[1]["messages"]
                .to_string()
                .matches(capability)
                .count(),
            1
        );
        assert!(!input.contains(capability));
        assert!(!entries[1].parts.iter().any(|part| {
            matches!(part, Part::Text(text) if text.contains("The user explicitly selected plugin files"))
        }));
        assert!(
            requests[1]["messages"]
                .to_string()
                .contains("Guidance folder instructions")
        );
        assert!(
            !requests[0]["messages"]
                .to_string()
                .contains("Guidance folder instructions")
        );
        assert!(
            requests[1]["messages"][0]["content"]
                .to_string()
                .contains("Guidance folder instructions")
        );
        assert_eq!(requests[0]["tools"], requests[1]["tools"]);
        assert_eq!(
            fixture.client.execute(first_request).await.unwrap(),
            first_result
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
        fixture.close().await;
    }
}

async fn configure(fixture: &mut Fixture, model: &str, effort: Effort) {
    let mut config = fixture.session.config.clone();
    config.model = model.into();
    config.effort = effort;
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
}

async fn fresh(fixture: &Fixture, active: &QueuedTurn, text: &str) -> QueuedTurn {
    let request = fixture.client.prepare(Command::GuideTurn {
        recipient: active.id,
        expected_revision: active.revision,
        message: text.into(),
    });
    let accepted = fixture.client.execute(request.clone()).await.unwrap();
    let Output::QueuedTurn(turn) = &accepted else {
        panic!("turn expected")
    };
    let queue = fixture.queue().await;
    assert_eq!(fixture.client.execute(request).await.unwrap(), accepted);
    assert_eq!(fixture.queue().await, queue);
    turn.clone()
}

fn declares(request: &serde_json::Value, name: &str) -> bool {
    request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["function"]["name"] == exposed(name))
}

#[tokio::test]
async fn queues_changed_captures_without_replacement() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let server = Server::markdown_held("Completed".into(), release.clone()).await;
        let mut fixture = Fixture::with_server(remote, server).await;
        fixture.reasoning().await;
        let active = fixture.push("Initial task").await;
        fixture.server.wait_count(1).await;
        let waiting = held(&fixture, "Keep this queue position".into()).await;
        configure(&mut fixture, "fixture-a", Effort::Low).await;
        let first = fresh(&fixture, &active, "Use the selected effort").await;
        assert_eq!(first.config, fixture.session.config);
        configure(&mut fixture, "fixture-b", Effort::Low).await;
        let selected = held(&fixture, "Original queued message".into()).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::EditQueuedTurn {
                turn: selected.id,
                expected_revision: 1,
                message: "Edited queued message".into(),
            }))
            .await
            .unwrap();
        let request = guide(&fixture, selected.id, active.id, 2);
        let accepted = fixture.client.execute(request.clone()).await.unwrap();
        let queue = fixture.queue().await;
        assert_eq!(
            queue.items.iter().map(|item| item.turn).collect::<Vec<_>>(),
            [waiting.id, first.id, selected.id]
        );
        assert_eq!(fixture.client.execute(request).await.unwrap(), accepted);
        assert_eq!(fixture.queue().await, queue);
        let saved = read(&fixture.client, selected.id).await;
        assert_eq!(saved.turn, selected);
        assert_eq!(saved.message.text, "Edited queued message");

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
        let last = fresh(&fixture, &active, "Use the enabled tools").await;
        assert!(!last.plugins.iter().any(|package| package.name == "files"));
        let page = history(&fixture.client, fixture.session.id).await;
        assert_eq!(status(&page, active.id), Status::Running);
        assert_eq!(
            page.queue
                .items
                .iter()
                .map(|item| item.turn)
                .collect::<Vec<_>>(),
            [waiting.id, first.id, selected.id, last.id]
        );
        for turn in [first.id, selected.id, last.id] {
            let item = page
                .queue
                .items
                .iter()
                .find(|item| item.turn == turn)
                .unwrap();
            assert!(item.ready);
            assert_eq!(item.guidance, None);
            assert_eq!(status(&page, turn), Status::Queued);
        }
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        release.cancel();
        let page = finished(&fixture.client, fixture.session.id, last.id).await;
        for turn in [active.id, first.id, selected.id, last.id] {
            assert_eq!(status(&page, turn), Status::Completed);
        }
        assert_eq!(page.queue.items.len(), 1);
        assert_eq!(page.queue.items[0].turn, waiting.id);
        let requests = fixture.server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[0]["reasoning_effort"], "high");
        assert_eq!(requests[0]["model"], "fixture-a");
        assert_eq!(requests[1]["model"], "fixture-a");
        for request in &requests[1..] {
            assert_eq!(request["reasoning_effort"], "low");
        }
        for request in &requests[2..] {
            assert_eq!(request["model"], "fixture-b");
        }
        let read_file = plugin_tool("files", "read_file");
        assert!(
            requests[..3]
                .iter()
                .all(|request| declares(request, &read_file))
        );
        assert!(!declares(&requests[3], &read_file));
        assert_eq!(prompt(&requests[2]), "Edited queued message");
        fixture.close().await;
    }
}

#[tokio::test]
async fn retains_changed_input_after_stop() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let server = Server::markdown_held("Completed".into(), release.clone()).await;
        let mut fixture = Fixture::with_server(remote, server).await;
        fixture.reasoning().await;
        let active = fixture.push("Initial task").await;
        fixture.server.wait_count(1).await;
        configure(&mut fixture, "fixture-b", Effort::Low).await;
        let queued = fresh(&fixture, &active, "Continue with selected settings").await;
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
        assert_eq!(status(&page, queued.id), Status::Queued);
        assert!(page.queue.paused);
        assert_eq!(page.queue.items.len(), 1);
        assert_eq!(page.queue.items[0].turn, queued.id);
        assert_eq!(page.queue.items[0].guidance, None);
        assert_eq!(read(&fixture.client, queued.id).await.turn, queued);
        release.cancel();
        fixture.pause(false).await;
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(status(&page, queued.id), Status::Completed);
        assert!(page.queue.items.is_empty());
        let requests = fixture.server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1]["model"], "fixture-b");
        assert_eq!(requests[1]["reasoning_effort"], "low");
        fixture.close().await;
    }
}
