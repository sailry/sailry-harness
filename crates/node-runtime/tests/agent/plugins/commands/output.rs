use super::*;
use sailry_protocol::process::OutputStream;
use sailry_protocol::tool::OutputReference;

async fn projected(server: &Server) -> Value {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let value = server
                .requests
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find_map(|request| {
                    request["messages"].as_array()?.iter().find_map(|message| {
                        (message["role"] == "tool")
                            .then(|| message["content"].as_str())
                            .flatten()
                            .and_then(|text| serde_json::from_str::<Value>(text).ok())
                            .filter(|value| value["output"]["call"].is_string())
                    })
                });
            if let Some(value) = value {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("model output projection deadline")
}

#[tokio::test]
async fn reads_large_failure_tail_at_original_offsets() {
    for remote in [false, true] {
        let prefix_bytes = 21 * 1024 * 1024;
        let failure = format!(
            "failures:\n{}last failure\n",
            "failed assertion details\n".repeat(2000)
        );
        assert!(failure.len() > 30 * 1024);
        let command = format!(
            "dd if=/dev/zero bs=1048576 count=21 2>/dev/null | tr '\\000' x; printf '%s' '{failure}'; exit 7"
        );
        let gate = Arc::new(tokio::sync::Notify::new());
        let server = Server::held_after_tool(
            vec![(name("run_command"), json!({"command":command}))],
            gate.clone(),
        )
        .await;
        let mut fixture = Fixture::packages(remote, &server, &["commands"]).await;
        planning::configure(&mut fixture, WorkMode::Code).await;
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Read the final failure details".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let projected = projected(&server).await;
        let reference: OutputReference =
            serde_json::from_value(projected["output"].clone()).unwrap();
        let total = prefix_bytes + failure.len();
        assert_eq!(projected["data"]["stdout"]["total_bytes"], total);
        let mut offset = prefix_bytes;
        let mut text = String::new();
        loop {
            let Output::CommandOutputPage(page) = execute(
                &fixture,
                Command::ReadToolOutput {
                    turn: turn.id,
                    output: reference.clone(),
                    stream: Some(OutputStream::Stdout),
                    offset,
                    limit: 4093,
                },
            )
            .await
            else {
                panic!("retained failure tail expected")
            };
            assert_eq!(page.offset, offset);
            assert_eq!(page.total_bytes, total as u64);
            assert!(page.truncated && !page.invalid_utf8);
            text.push_str(&page.text);
            match page.next_offset {
                Some(next) => {
                    assert!(next > offset);
                    offset = next;
                }
                None => break,
            }
        }
        assert_eq!(text, failure);
        gate.notify_one();
        finished(&fixture.client, fixture.session.id, turn.id).await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_business_ids_as_command_streams() {
    for remote in [false, true] {
        let gate = Arc::new(tokio::sync::Notify::new());
        let server = Server::held_after_tool(vec![(name("run_command"),json!({
            "command":"i=0; while [ \"$i\" -lt 4000 ]; do printf 'original command evidence\\n'; i=$((i+1)); done"
        }))],gate.clone()).await;
        let mut fixture = Fixture::packages(remote, &server, &["commands"]).await;
        planning::configure(&mut fixture, WorkMode::Code).await;
        let Output::QueuedTurn(turn) = execute(
            &fixture,
            Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Inspect retained output".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        let projected = projected(&server).await;
        let source: OutputReference = serde_json::from_value(projected["output"].clone()).unwrap();
        let id: RequestId = projected["sailry_command"]["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let business = json!({"kind":"business_record","data":{"info":{"id":id},"text":"Preserved business output"}});
        let replacement = business.clone();
        let call = source.call.clone();
        let turn_id = turn.id;
        // Seed a canonical business result that happens to name a live command.
        // The dispatch must use the result kind rather than that coincidental ID.
        fixture.node.with_storage(move |db| Box::pin(async move {
            let mut rows = db.prepare("SELECT sequence,body FROM agent_events WHERE turn=?1 ORDER BY sequence")
                .await.map_err(crate::database_fixture::fault)?
                .query([turn_id.to_string()]).await.map_err(crate::database_fixture::fault)?;
            let mut replacement_event = None;
            while let Some(row) = rows.next().await.map_err(crate::database_fixture::fault)? {
                let sequence: i64 = row.get(0).map_err(crate::database_fixture::fault)?;
                let body: Vec<u8> = row.get(1).map_err(crate::database_fixture::fault)?;
                let mut event: adk_core::Event = serde_json::from_slice(&body).unwrap();
                let mut changed = false;
                if let Some(content) = event.llm_response.content.as_mut() {
                    for part in &mut content.parts {
                        if let adk_core::Part::FunctionResponse {id:Some(id),function_response,..} = part
                            && id == &call
                        {
                            function_response.response = replacement.clone();
                            changed = true;
                            break;
                        }
                    }
                }
                if changed {
                    replacement_event = Some((sequence,serde_json::to_vec(&event).unwrap()));
                }
            }
            drop(rows);
            let (sequence,body) = replacement_event.expect("canonical command result");
            db.execute("UPDATE agent_events SET body=?1 WHERE sequence=?2",turso::params![body,sequence])
                .await.map_err(crate::database_fixture::fault)?;
            Ok(())
        })).await.unwrap();
        let output = OutputReference {
            call: source.call,
            revision: blake3::hash(business.to_string().as_bytes())
                .to_hex()
                .to_string(),
        };
        let error = fixture
            .client
            .execute(fixture.client.prepare(Command::ReadToolOutput {
                turn: turn.id,
                output: output.clone(),
                stream: Some(OutputStream::Stdout),
                offset: 0,
                limit: 4096,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.message, "tool result has no retained command stream");
        let Output::ToolOutput(page) = execute(
            &fixture,
            Command::ReadToolOutput {
                turn: turn.id,
                output,
                stream: None,
                offset: 0,
                limit: 4096,
            },
        )
        .await
        else {
            panic!("canonical business output expected")
        };
        assert_eq!(page.text, business.to_string());
        assert!(page.next_offset.is_none());
        let Output::CommandOutputPage(page) = execute(
            &fixture,
            Command::ReadCommandPage {
                session: fixture.session.id,
                id,
                stream: OutputStream::Stdout,
                offset: 0,
                limit: 4096,
            },
        )
        .await
        else {
            panic!("actual command stream expected")
        };
        assert!(page.text.starts_with("original command evidence"));
        gate.notify_one();
        finished(&fixture.client, fixture.session.id, turn.id).await;
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn retains_foreground_streams_and_canonical_results() {
    for remote in [false, true] {
        let gate = Arc::new(tokio::sync::Notify::new());
        let server = Server::held_after_tool(vec![(name("run_command"),json!({
            "command":"printf '\\033[31mhead\\033[0m\\n'; i=0; while [ \"$i\" -lt 4000 ]; do printf 'preserved Unicode 中文🙂 output row\\n'; i=$((i+1)); done; printf 'last failure\\n'; exit 7"
        }))],gate.clone()).await;
        let mut fixture = Fixture::packages(remote, &server, &["commands"]).await;
        planning::configure(&mut fixture, WorkMode::Code).await;
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Write the requested file".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let turn = turn.id;
        let projected = projected(&server).await;
        assert!(
            projected["data"]["stdout"]["text"]
                .as_str()
                .unwrap()
                .ends_with("last failure\n")
        );
        assert!(
            !projected["data"]["stdout"]["text"]
                .as_str()
                .unwrap()
                .contains('\u{1b}')
        );
        let reference: OutputReference =
            serde_json::from_value(projected["output"].clone()).unwrap();
        let mut text = String::new();
        let mut offset = 0;
        loop {
            let Output::ToolOutput(page) = execute(
                &fixture,
                Command::ReadToolOutput {
                    turn,
                    output: reference.clone(),
                    stream: None,
                    offset,
                    limit: 997,
                },
            )
            .await
            else {
                panic!("original tool result page expected")
            };
            text.push_str(&page.text);
            let Some(next) = page.next_offset else {
                break;
            };
            offset = next;
        }
        let original: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            original["data"]["outcome"],
            json!({"kind":"exited","data":7})
        );
        assert!(
            original["data"]["stdout"]["text"]
                .as_str()
                .unwrap()
                .contains('\u{1b}')
        );
        let id = serde_json::from_value(original["sailry_command"]["id"].clone()).unwrap();
        assert!(
            matches!(execute(&fixture,Command::ListCommands {session:fixture.session.id}).await,Output::Commands(commands) if commands.is_empty())
        );
        let mut complete = String::new();
        let mut offset = 0;
        loop {
            let Output::CommandOutputPage(page) = execute(
                &fixture,
                Command::ReadToolOutput {
                    turn,
                    output: reference.clone(),
                    stream: Some(OutputStream::Stdout),
                    offset,
                    limit: 1013,
                },
            )
            .await
            else {
                panic!("retained stream expected")
            };
            assert!(!page.truncated && !page.invalid_utf8);
            complete.push_str(&page.text);
            let Some(next) = page.next_offset else {
                break;
            };
            offset = next;
        }
        assert_eq!(complete.lines().count(), 4002);
        assert!(complete.starts_with("\u{1b}[31mhead\u{1b}[0m\n"));
        assert!(complete.ends_with("last failure\n"));
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadCommandPage {
                    session: SessionId::new(),
                    id,
                    stream: OutputStream::Stdout,
                    offset: 0,
                    limit: 100,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let mut stale = reference.clone();
        stale.revision = "0".repeat(64);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ReadToolOutput {
                    turn,
                    output: stale,
                    stream: None,
                    offset: 0,
                    limit: 100,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(profile).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::Session(session) = client
            .execute(client.prepare(Command::ReadSession {
                session: fixture.session.id,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(resumed) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Inspect the earlier output".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("new invocation expected")
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&client, session.id).await;
                if page
                    .runs
                    .iter()
                    .any(|run| run.turn == resumed.id && run.status == Status::Running)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let Output::ToolOutput(saved) = client
            .execute(client.prepare(Command::ReadToolOutput {
                turn: resumed.id,
                output: reference,
                stream: None,
                offset: 0,
                limit: 100,
            }))
            .await
            .unwrap()
        else {
            panic!("persisted canonical output expected")
        };
        assert!(text.starts_with(&saved.text));
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadCommandPage {
                    session: session.id,
                    id,
                    stream: OutputStream::Stdout,
                    offset: 0,
                    limit: 100,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        gate.notify_waiters();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn refreshes_git_after_command_completion() {
    for remote in [false, true] {
        for background in [false, true] {
            let command = if background {
                "printf ready; IFS= read -r value; printf changed > command-created.txt"
            } else {
                "printf changed > command-created.txt"
            };
            let server = Server::tools(vec![(
                name("run_command"),
                json!({
                    "command":command,"access":"unrestricted","background":background
                }),
            )])
            .await;
            let mut fixture = Fixture::packages(remote, &server, &["commands"]).await;
            let mut config = fixture.session.config.clone();
            config.mode = WorkMode::Code;
            config.permission = Permission::Ask;
            let Output::Session(session) = execute(
                &fixture,
                Command::SetSessionConfig {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    config,
                },
            )
            .await
            else {
                panic!("session expected")
            };
            fixture.session = session;
            git2::Repository::init(&fixture.root).unwrap();
            let turn = approvals::rules::submit(&fixture.client, &fixture.session)
                .await
                .id;
            let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
            if !background {
                execute(
                    &fixture,
                    Command::InspectGit {
                        worktree: fixture.session.worktree,
                    },
                )
                .await;
            }
            process::decide(&fixture.client, &approval, Decision::Approve).await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            if background {
                let results = results(&fixture.client, &page).await;
                let id = serde_json::from_value(results[0]["data"]["id"].clone()).unwrap();
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let Output::CommandOutput(snapshot) = execute(
                            &fixture,
                            Command::ReadCommand {
                                session: fixture.session.id,
                                id,
                            },
                        )
                        .await
                        else {
                            panic!("command snapshot expected")
                        };
                        if snapshot.stdout.text.contains("ready") {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                execute(
                    &fixture,
                    Command::InspectGit {
                        worktree: fixture.session.worktree,
                    },
                )
                .await;
                execute(
                    &fixture,
                    Command::WriteCommand {
                        session: fixture.session.id,
                        id,
                        input: "finish\n".into(),
                    },
                )
                .await;
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let Output::CommandOutput(snapshot) = execute(
                            &fixture,
                            Command::ReadCommand {
                                session: fixture.session.id,
                                id,
                            },
                        )
                        .await
                        else {
                            panic!("command snapshot expected")
                        };
                        if matches!(
                            snapshot.info.status,
                            sailry_protocol::process::Status::Finished(_)
                        ) {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
            }
            let Output::GitStatus(status) = execute(
                &fixture,
                Command::InspectGit {
                    worktree: fixture.session.worktree,
                },
            )
            .await
            else {
                panic!("Git status expected")
            };
            assert!(
                status
                    .entries
                    .iter()
                    .any(|entry| entry.path == "command-created.txt")
            );
            fixture.node.shutdown().await.unwrap();
        }
    }
}
