//! Opt-in real-model token measurements for identical synthetic revision comparisons.
use super::*;
use serde_json::json;

const CALL_LIMIT: usize = 8;
const MARKER: &str = "FIRST=Sailry-synthetic-token-acceptance";

#[tokio::test]
#[ignore = "requires a stopped native acceptance profile, provider and model; incurs model usage"]
async fn reports_token_usage() {
    let model = std::env::var("SAILRY_ACCEPTANCE_MODEL").expect("explicit model required");
    let provider = std::env::var("SAILRY_ACCEPTANCE_PROVIDER_ID")
        .expect("explicit provider required")
        .parse()
        .unwrap();
    let variant = std::env::var("SAILRY_ACCEPTANCE_VARIANT").expect("revision label required");
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("AGENTS.md"),
            "This is a synthetic acceptance project. Preserve its files and report findings concisely.\n").unwrap();
        let padding = "Synthetic acceptance padding. ".repeat(18);
        let mut source = format!("{MARKER}\n");
        for index in 0..128 {
            source.push_str(&format!("{index:03} {padding}\n"));
        }
        std::fs::write(root.join("synthetic.txt"), &source).unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let identity = node.id();
        node.shutdown().await.unwrap();
        let selected = provider_fixture::seed(&model, provider, &profile, identity).await;
        assert_eq!(selected.authentication, Authentication::ApiKey);
        assert!(matches!(
            selected.api,
            ModelApi::Responses | ModelApi::ChatCompletions | ModelApi::Anthropic
        ));
        let node = Node::start_empty(&profile).await.unwrap();
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
        approvals::install(&client, &["files"]).await;
        let (session, _) = configured_session(&client, &selected.endpoint, &root).await;
        let mut config = session.config;
        config.provider = selected.id;
        config.model = model.clone();
        config.credential = selected.credential;
        config.effort = Effort::Low;
        config.permission = Permission::Full;
        let Output::Session(session) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        println!(
            "{}",
            json!({"suite":"token-acceptance","stage":"start","variant":variant,
            "model":model,"api":selected.api,"remote":remote})
        );
        let started = std::time::Instant::now();
        let Output::QueuedTurn(turn) = client.execute(client.prepare(Command::SubmitTurn {
            session: session.id, expected_revision: session.revision,
            message: "Use read_file exactly once to read synthetic.txt with offset=0 and limit=2000. Return only the FIRST marker line from that result. The rest of the file is irrelevant. Preserve all files and do not use any other tools.".into(),
        })).await.unwrap() else { panic!("turn expected") };
        let outcome = tokio::time::timeout(
            Duration::from_secs(180),
            observe(&client, session.id, turn.id),
        )
        .await
        .map_err(|_| "real model deadline exceeded".to_owned())
        .and_then(|result| result);
        if outcome.is_err() {
            let _ = client
                .execute(client.prepare(Command::StopTurn { turn: turn.id }))
                .await;
        }
        let records = events(&node, turn.id).await;
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        if let Err(error) = &outcome {
            let observations: Vec<_> = records
                .iter()
                .filter_map(|event| {
                    let event = serde_json::to_value(event).unwrap();
                    event["event_metadata"]["sailry_model_observation"]
                        .as_str()
                        .map(|value| serde_json::from_str::<serde_json::Value>(value).unwrap())
                })
                .collect();
            println!(
                "{}",
                json!({"suite":"token-acceptance","stage":"failed",
                "variant":variant,"model":model,"remote":remote,"error":error,
                "observations":observations})
            );
        }
        let requests = observations(&records);
        let trace = trace(&records);
        let response_shapes: Vec<_> = records.iter().filter_map(|event|event.content())
            .flat_map(|content|&content.parts).filter_map(|part|match part {
                adk_core::Part::FunctionResponse {function_response,..} => Some(json!({
                    "name":function_response.name,
                    "kind":function_response.response.get("kind"),
                    "keys":function_response.response.as_object().map(|value|value.keys().collect::<Vec<_>>())
                })), _=>None,
            }).collect();
        println!(
            "{}",
            json!({"suite":"token-acceptance","stage":"observed","variant":variant,
            "model":model,"remote":remote,"requests":requests,"response_shapes":response_shapes,
            "tool_trace":trace})
        );
        let page = outcome.expect("bounded real-model task failed").page;
        assert_eq!(
            std::fs::read_to_string(root.join("synthetic.txt")).unwrap(),
            source
        );
        let answer: String = page
            .entries
            .iter()
            .filter(|entry| entry.author != "user")
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            answer.contains(MARKER),
            "model must report the actual marker"
        );
        let mut raw_bytes = 0;
        for part in records
            .iter()
            .filter_map(|event| event.content())
            .flat_map(|content| &content.parts)
        {
            match part {
                adk_core::Part::FunctionResponse {
                    function_response, ..
                } => {
                    let original = &function_response.response;
                    raw_bytes += original.to_string().len();
                    if function_response.name != plugin_tool("files", "read_file") {
                        continue;
                    }
                    let restored = file_text(original);
                    assert_eq!(
                        restored, source,
                        "canonical result must retain the complete read"
                    );
                }
                _ => {}
            }
        }
        assert_eq!(
            trace,
            vec![json!({"name":plugin_tool("files","read_file"),
            "arguments":{"path":"synthetic.txt","offset":0,"limit":2000}})]
        );
        assert_eq!(
            requests.len(),
            2,
            "one tool call requires two model generations"
        );
        let mut usage = Vec::new();
        for request in requests {
            assert_eq!(request.attempts.len(), 1, "acceptance must not retry");
            let attempt = &request.attempts[0];
            let actual = attempt
                .usage
                .as_ref()
                .expect("actual provider token usage required");
            usage.push(json!({"input":actual.input,"output":actual.output,
                "cached_input":actual.cached_input,"reasoning":actual.reasoning,
                "cache_reported":attempt.cache_reported,"transport":attempt.transport,
                "complete_context":attempt.complete_context,"estimated_categories":attempt.categories}));
        }
        println!(
            "{}",
            json!({"suite":"token-acceptance","passed":true,"variant":variant,
            "model":model,"remote":remote,"tool_trace":trace,"source_bytes":source.len(),
            "source_digest":blake3::hash(source.as_bytes()).to_hex().to_string(),
            "raw_result_bytes":raw_bytes,"page_bytes":serde_json::to_vec(&page).unwrap().len(),
            "elapsed_ms":started.elapsed().as_millis(),"responses":usage})
        );
    }
}

fn observations(
    records: &[adk_core::Event],
) -> Vec<sailry_protocol::conversation::observation::Request> {
    let mut observations = std::collections::BTreeMap::new();
    for event in records {
        if let Some(value) = event.provider_metadata.get("sailry_model_observation") {
            let request: sailry_protocol::conversation::observation::Request =
                serde_json::from_str(value).expect("canonical request observation");
            if !request.attempts.is_empty() {
                observations.insert(request.id.clone(), request);
            }
        }
    }
    let mut requests: Vec<_> = observations.into_values().collect();
    requests.sort_by_key(|request| request.timestamp_ms);
    requests
}

fn trace(records: &[adk_core::Event]) -> Vec<serde_json::Value> {
    records
        .iter()
        .filter_map(|event| event.content())
        .flat_map(|content| &content.parts)
        .filter_map(|part| match part {
            adk_core::Part::FunctionCall { name, args, .. } => {
                Some(json!({"name":name,"arguments":args}))
            }
            _ => None,
        })
        .collect()
}

fn file_text(value: &serde_json::Value) -> String {
    let page: sailry_protocol::FileLines =
        serde_json::from_value(value.get("data").unwrap_or(value).clone())
            .expect("canonical file lines from native or script-backed host operation");
    assert!(
        page.next_offset.is_none(),
        "synthetic read must be complete"
    );
    page.lines.into_iter().map(|line| line.text).collect()
}

#[tokio::test]
async fn extracts_file_and_usage() {
    for remote in [false, true] {
        let server = Server::compaction(false).await;
        let fixture = process::Fixture::files(remote, &server).await;
        let source = format!(
            "{MARKER}\n{}",
            "Synthetic parser fixture evidence.\n".repeat(120)
        );
        std::fs::write(fixture.root.join("source.txt"), &source).unwrap();
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Read source.txt and report the result".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let records = events(&fixture.node, turn.id).await;
        let requests = observations(&records);
        assert_eq!(requests.len(), 2);
        for request in requests {
            assert_eq!(request.attempts.len(), 1);
            let usage = request.attempts[0]
                .usage
                .as_ref()
                .expect("fixture provider usage");
            assert_eq!(usage.input, 100);
            assert_eq!(usage.output, 10);
        }
        assert_eq!(trace(&records).len(), 1);
        let read = records
            .iter()
            .filter_map(|event| event.content())
            .flat_map(|content| &content.parts)
            .find_map(|part| match part {
                adk_core::Part::FunctionResponse {
                    function_response, ..
                } if function_response.name == plugin_tool("files", "read_file") => {
                    Some(&function_response.response)
                }
                _ => None,
            })
            .expect("actual canonical file result");
        assert_eq!(file_text(read), source);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

async fn events(node: &Node, turn: TurnId) -> Vec<adk_core::Event> {
    node.with_storage(move |connection| {
        Box::pin(async move {
            let mut rows = connection
                .query(
                    "SELECT body FROM agent_events WHERE turn=?1 ORDER BY sequence",
                    [turn.to_string()],
                )
                .await
                .map_err(crate::database_fixture::fault)?;
            let mut events = Vec::new();
            while let Some(row) = rows.next().await.map_err(crate::database_fixture::fault)? {
                events.push(serde_json::from_slice(&row.get::<Vec<u8>>(0).unwrap()).unwrap());
            }
            Ok(events)
        })
    })
    .await
    .unwrap()
}

async fn observe(
    client: &Client,
    session: SessionId,
    turn: TurnId,
) -> Result<sailry_protocol::conversation::History, String> {
    loop {
        let output = client
            .execute(client.prepare(Command::ReadConversation {
                session,
                before: None,
                limit: 100,
            }))
            .await
            .map_err(|_| "conversation read failed".to_owned())?;
        let Output::Conversation(value) = output else {
            return Err("conversation expected".into());
        };
        let page = &value.page;
        let calls = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter(|part| matches!(part, Part::ToolCall { .. }))
            .count();
        if calls > CALL_LIMIT {
            return Err("real model tool budget exceeded".into());
        }
        if let Some(run) = page.runs.iter().find(|run| run.turn == turn)
            && !matches!(
                run.status,
                Status::Queued | Status::Running | Status::Stopping
            )
        {
            return if run.status == Status::Completed {
                Ok(value)
            } else {
                Err(format!(
                    "real model task ended with {:?}: {:?}",
                    run.status, run.error
                ))
            };
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
