use super::*;
use serde_json::{Value, json};

#[path = "prompt/guides.rs"]
mod guides;
#[path = "prompt/personal.rs"]
mod personal;

const BASE: &str = include_str!("../../../../src/agent/prompt.txt");
const ROOT_RULE: &str = "Root fixture rule: preserve the requested project scope";
const NESTED_RULE: &str = "Nested fixture rule: retain the file's Unicode content";
const FIRST: &str = "Read the requested fixture file without changing it";
const SECOND: &str = "Continue the same task after restoring the session";
const PERSONAL_FIRST: &str = "Personal fixture preference: preserve concise answers";
const PERSONAL_NEXT: &str = "Updated personal fixture preference: explain verified outcomes";
const CONTROLLER_RULE: &str = "Controller preferences must not become execution instructions";

fn prepare(fixture: &Fixture) {
    std::fs::write(fixture.root.join("CLAUDE.md"), ROOT_RULE).unwrap();
    std::fs::create_dir(fixture.root.join("nested")).unwrap();
    std::fs::write(fixture.root.join("nested/GEMINI.md"), NESTED_RULE).unwrap();
    std::fs::write(
        fixture.node.profile().join("instructions.md"),
        PERSONAL_FIRST,
    )
    .unwrap();
    std::fs::write(
        fixture.directory.path().join("controller/instructions.md"),
        CONTROLLER_RULE,
    )
    .unwrap();
    for path in ["native.txt", "nested/native.txt"] {
        std::fs::write(fixture.root.join(path), "工具内容 🙂").unwrap();
    }
}

fn referenced(task: &str, directory: bool) -> Input {
    Input {
        text: task.into(),
        attachments: vec![],
        references: vec![reference::Reference {
            label: "Nested fixture".into(),
            target: if directory {
                reference::Target::Directory("nested".into())
            } else {
                reference::Target::File("nested/native.txt".into())
            },
        }],
    }
}

async fn resume(fixture: Fixture, first: Input, second: Input, remote: bool) {
    let Output::QueuedTurn(turn) = execute(
        &fixture.client,
        Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: first,
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    let page = finished(&fixture.client, fixture.session.id, turn.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    assert!(
        page.entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .any(|part| matches!(part, Part::ToolResult { .. }))
    );
    let Output::QueuedTurn(queued) = execute(
        &fixture.client,
        Command::QueueTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: second,
        },
    )
    .await
    else {
        panic!("turn expected")
    };
    std::fs::write(
        fixture.node.profile().join("instructions.md"),
        PERSONAL_NEXT,
    )
    .unwrap();
    let profile = fixture.node.profile().to_owned();
    fixture.node.shutdown().await.unwrap();
    let node = Node::start_empty(profile).await.unwrap();
    if fixture.provider.api == ModelApi::Responses {
        node.set_model_transport_fixture(adk_model::openai::WsOptions {
            enabled: std::sync::Arc::new(|| false),
            ..Default::default()
        });
    }
    let client = Client::new(if remote {
        fixture.controller.handle().remote(node.link().address())
    } else {
        node.local()
    });
    execute(&client, Command::StartQueuedTurn { turn: queued.id }).await;
    let queue = history(&client, fixture.session.id).await.queue;
    assert!(queue.paused);
    execute(
        &client,
        Command::SetQueuePaused {
            session: fixture.session.id,
            expected_revision: queue.revision,
            paused: false,
        },
    )
    .await;
    let page = finished(&client, fixture.session.id, queued.id).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{page:?}"
    );
    assert!(page.entries.iter().all(|entry| entry.parts.iter().all(|part| {
        !matches!(part, Part::Text(text) if text.contains("Node context") || text.contains(ROOT_RULE) || text.contains(PERSONAL_FIRST))
    })));
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

fn declarations(body: &Value) -> Vec<String> {
    let mut declarations = body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|tool| {
            tool.get("functionDeclarations")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_else(|| vec![tool.clone()])
        })
        .map(|declaration| declaration.to_string())
        .collect::<Vec<_>>();
    declarations.sort();
    declarations
}

fn content(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.into();
    }
    value
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn instruction(api: ModelApi, body: &Value) -> String {
    match api {
        ModelApi::Anthropic => content(&body["system"]),
        ModelApi::Gemini => {
            assert!(body.get("systemInstruction").is_none());
            assert_eq!(body["contents"][0]["role"], "user");
            content(&body["contents"][0]["parts"])
        }
        ModelApi::Responses => content(&body["instructions"]),
        ModelApi::ChatCompletions | ModelApi::DeepSeek => body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "system")
            .map(|message| content(&message["content"]))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => unreachable!(),
    }
}

fn users(api: ModelApi, body: &Value) -> Vec<String> {
    let (messages, parts) = match api {
        ModelApi::Gemini => (&body["contents"], "parts"),
        ModelApi::Responses => (&body["input"], "content"),
        _ => (&body["messages"], "content"),
    };
    messages
        .as_array()
        .unwrap()
        .iter()
        .skip(usize::from(api == ModelApi::Gemini))
        .filter(|message| message["role"] == "user")
        .map(|message| content(&message[parts]))
        .collect()
}

fn ordered_tools(api: ModelApi, body: &Value) -> Value {
    let mut tools = body["tools"].clone();
    match api {
        ModelApi::Gemini => {
            for tool in tools.as_array_mut().unwrap() {
                if let Some(declarations) = tool["functionDeclarations"].as_array_mut() {
                    declarations
                        .sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
                }
            }
        }
        ModelApi::DeepSeek => tools.as_array_mut().unwrap().sort_by(|left, right| {
            left["function"]["name"]
                .as_str()
                .cmp(&right["function"]["name"].as_str())
        }),
        _ => unreachable!(),
    }
    tools
}

fn assert_policy(api: ModelApi, body: &Value, resumed: bool, nested: bool) {
    let system = instruction(api, body);
    assert_eq!(system.matches(BASE.trim()).count(), 1, "{api:?}");
    for dynamic in [
        ROOT_RULE,
        NESTED_RULE,
        PERSONAL_FIRST,
        PERSONAL_NEXT,
        CONTROLLER_RULE,
        "This turn uses Ask permission",
        "Work mode\n",
        "Node environment",
    ] {
        assert!(!system.contains(dynamic), "{api:?}: {dynamic}");
    }
    let history = body.to_string();
    for (context, expected) in [
        (ROOT_RULE, 1),
        (NESTED_RULE, usize::from(nested)),
        (PERSONAL_FIRST, 1),
        (PERSONAL_NEXT, usize::from(resumed)),
        (CONTROLLER_RULE, 0),
    ] {
        assert_eq!(
            history.matches(context).count(),
            expected,
            "{api:?}: {context}"
        );
    }
    let users = users(api, body);
    assert!(
        users.iter().all(|text| !text.contains(BASE.trim())),
        "{api:?}"
    );
    assert!(
        users.iter().all(|text| !text.contains(CONTROLLER_RULE)),
        "{api:?}"
    );
    for (task, expected) in [(FIRST, 1), (SECOND, usize::from(resumed))] {
        assert!(!system.contains(task), "{api:?}: user task became policy");
        assert_eq!(
            users
                .iter()
                .map(|text| text.matches(task).count())
                .sum::<usize>(),
            expected,
            "{api:?}: {task}"
        );
    }
}

#[tokio::test]
async fn preserves_native_roles_across_resume() {
    for api in [ModelApi::Anthropic, ModelApi::Gemini, ModelApi::DeepSeek] {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Tool).await;
            let fixture = Fixture::packages(remote, api, &server.endpoint, &["files"]).await;
            prepare(&fixture);
            resume(
                fixture,
                referenced(FIRST, remote),
                referenced(SECOND, remote),
                remote,
            )
            .await;
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3, "{api:?}, remote={remote}");
            for (index, request) in requests.iter().enumerate() {
                assert_policy(api, &request.body, index == 2, true);
                assert_eq!(
                    instruction(api, &request.body),
                    instruction(api, &requests[0].body),
                    "{api:?}, remote={remote}, request={index}"
                );
                if matches!(api, ModelApi::Gemini | ModelApi::DeepSeek) && index == 2 {
                    // These SDKs do not persist HashMap iteration across Node restarts.
                    assert_eq!(
                        ordered_tools(api, &request.body),
                        ordered_tools(api, &requests[0].body),
                        "{api:?}, remote={remote}, request={index}"
                    );
                } else {
                    assert_eq!(
                        request.body["tools"], requests[0].body["tools"],
                        "{api:?}, remote={remote}, request={index}"
                    );
                }
            }
            assert!(requests[1].body.to_string().contains("工具内容 🙂"));
            assert!(requests[2].body.to_string().contains(server::ANSWER));
        }
    }
}

#[tokio::test]
async fn loads_rules_at_openai_tool_boundary() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        for remote in [false, true] {
            let name = crate::agent_support::plugin_tool("files", "read_file");
            let arguments = json!({"path":"nested/native.txt"});
            let server = if api == ModelApi::Responses {
                crate::agent_support::Server::phases_with_tool(&name, arguments).await
            } else {
                crate::agent_support::Server::tools(vec![(name, arguments)]).await
            };
            let fixture = Fixture::packages(remote, api, &server.endpoint, &["files"]).await;
            prepare(&fixture);
            resume(fixture, FIRST.into(), referenced(SECOND, false), remote).await;
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3, "{api:?}, remote={remote}");
            for (index, request) in requests.iter().enumerate() {
                assert_policy(api, request, index == 2, index > 0);
                assert_eq!(instruction(api, request), instruction(api, &requests[0]));
                assert_eq!(request["tools"], requests[0]["tools"]);
            }
            assert!(requests[1].to_string().contains("工具内容 🙂"));
        }
    }
}

#[tokio::test]
async fn preserves_native_summary_instructions() {
    for remote in [false, true] {
        for api in [ModelApi::Anthropic, ModelApi::Gemini] {
            let evidence = format!("{}\n{}", server::ANSWER,
                "Verified project/src/module.rs and its callers; the public contract remains unchanged.\n".repeat(128));
            let server = Server::start(api, Reply::Compaction(evidence.clone())).await;
            let fixture = Fixture::empty(remote, api, &server.endpoint).await;
            let task = "Original compaction constraint 🙂 ".repeat(40);
            let turn = fixture.submit(&task).await;
            let before = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                before.runs.last().unwrap().status,
                Status::Completed,
                "{before:?}"
            );
            assert_eq!(
                before.entries.last().unwrap().parts,
                [Part::Text(evidence.clone())]
            );
            let usage = before.entries.last().unwrap().usage.as_ref().unwrap();
            assert_eq!((usage.input, usage.output), (8, 4));
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            let normal = server.requests.lock().unwrap()[0].body.clone();
            let focus = "Preserve exact paths and unfinished work";
            let Output::QueuedTurn(compact) = execute(
                &fixture.client,
                Command::CompactContext {
                    focus: Some(focus.into()),
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                },
            )
            .await
            else {
                panic!("compaction turn expected");
            };
            assert_eq!(compact.kind, RunKind::Compaction);
            assert_eq!(compact.config.effort, fixture.session.config.effort);
            let page = finished(&fixture.client, fixture.session.id, compact.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
            assert_eq!(&page.entries[..before.entries.len()], before.entries);
            let entry = page.entries.last().unwrap();
            assert_eq!(entry.turn, compact.id);
            assert_eq!(entry.author, "system");
            assert!(entry.usage.is_none());
            let Part::Compaction(visible) = &entry.parts[0] else {
                panic!("summary expected");
            };
            assert!(visible.contains(&task));
            assert!(!visible.contains(&evidence));
            let id = entry.id.clone();
            let stored = fixture
                .node
                .with_storage(move |db| {
                    Box::pin(async move {
                        let body: Vec<u8> = db
                            .prepare("SELECT body FROM agent_events WHERE id=?1")
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .query_row([id])
                            .await
                            .map_err(crate::database_fixture::fault)?
                            .get(0)
                            .map_err(crate::database_fixture::fault)?;
                        serde_json::from_slice::<adk_core::Event>(&body)
                            .map_err(crate::database_fixture::fault)
                    })
                })
                .await
                .unwrap();
            let handoff = stored.actions.compaction.unwrap().compacted_content;
            assert_eq!(handoff.role, "user");
            let generated = handoff.parts[0].text().unwrap();
            assert!(generated.starts_with("## Goals\n"));
            assert!(generated.contains(&task));
            assert!(!generated.contains("Captured Node context"));
            assert_eq!(handoff.parts.len(), 1);
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), 2);
                let summary = &requests[1].body;
                let (messages, ordinary, parts) = if api == ModelApi::Gemini {
                    assert_eq!(
                        requests[1].path,
                        "/v1/models/fixture-a:streamGenerateContent?alt=sse"
                    );
                    assert_eq!(summary["generationConfig"]["maxOutputTokens"], 512);
                    assert_eq!(
                        summary["generationConfig"]["thinkingConfig"],
                        normal["generationConfig"]["thinkingConfig"]
                    );
                    (&summary["contents"], &normal["contents"], "parts")
                } else {
                    assert_eq!(summary["model"], normal["model"]);
                    assert_eq!(summary["max_tokens"], 512);
                    assert_eq!(summary["thinking"], normal["thinking"]);
                    assert_eq!(summary["system"], normal["system"]);
                    (&summary["messages"], &normal["messages"], "content")
                };
                let messages = messages.as_array().unwrap();
                let ordinary = ordinary.as_array().unwrap();
                assert_eq!(&messages[..ordinary.len()], ordinary);
                if api == ModelApi::Gemini {
                    // ADK builds Gemini declarations from an unordered map.
                    assert_eq!(declarations(summary), declarations(&normal));
                    let mut summary = summary.clone();
                    let mut normal = normal.clone();
                    for body in [&mut summary, &mut normal] {
                        body.as_object_mut().unwrap().remove("tools");
                        body.as_object_mut().unwrap().remove("generationConfig");
                        body["contents"]
                            .as_array_mut()
                            .unwrap()
                            .truncate(ordinary.len());
                    }
                    assert_eq!(summary, normal);
                } else {
                    assert_eq!(summary["tools"], normal["tools"]);
                }
                let request = messages.last().unwrap();
                assert_eq!(request["role"], "user");
                let instruction = content(&request[parts]);
                assert!(instruction.starts_with("Update the existing handoff"));
                assert!(instruction.contains("Summary output budget: 512 tokens"));
                assert!(instruction.contains(focus));
                assert!(!instruction.contains(&task));
                for heading in [
                    "## Goals",
                    "## User constraints",
                    "## Key facts",
                    "## Changed files and reasons",
                    "## Errors and fixes",
                    "## Todo",
                    "## Current progress",
                    "## Next steps",
                ] {
                    assert!(instruction.contains(heading), "{heading}");
                }
                assert!(
                    messages[..messages.len() - 1]
                        .iter()
                        .any(|message| content(&message[parts]).contains(&evidence))
                );
            }
            let profile = fixture.node.profile().to_owned();
            fixture.node.shutdown().await.unwrap();
            let node = Node::start_empty(profile).await.unwrap();
            let client = Client::new(if remote {
                fixture.controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            assert_eq!(history(&client, fixture.session.id).await, page);
            let Output::QueuedTurn(next) = execute(
                &client,
                Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: "Continue with the saved handoff".into(),
                },
            )
            .await
            else {
                panic!("turn expected");
            };
            let resumed = finished(&client, fixture.session.id, next.id).await;
            assert_eq!(
                resumed.runs.last().unwrap().status,
                Status::Completed,
                "{resumed:?}"
            );
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            let resumed = &requests[2].body;
            let (messages, parts) = if api == ModelApi::Gemini {
                (&resumed["contents"], "parts")
            } else {
                (&resumed["messages"], "content")
            };
            let matching = messages
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| content(&message[parts]).contains(generated))
                .collect::<Vec<_>>();
            assert_eq!(matching.len(), 1);
            assert_eq!(matching[0]["role"], "user");
            // Gemini's pinned adapter carries the preamble as a leading user
            // message; Anthropic keeps it in its separate system field.
            let messages = messages.as_array().unwrap();
            let first = messages
                .iter()
                .position(|message| content(&message[parts]).contains(generated))
                .unwrap();
            let preamble = usize::from(api == ModelApi::Gemini);
            if preamble != 0 {
                assert_eq!(messages[0], normal["contents"][0]);
            }
            assert!(messages[preamble..first].iter().all(|message| {
                content(&message[parts]).contains("Captured Node context (verbatim)\n")
            }));
            assert_eq!(
                messages
                    .iter()
                    .map(|message| content(&message[parts])
                        .matches("Captured Node context (verbatim)\n")
                        .count())
                    .sum::<usize>(),
                1
            );
            drop(requests);
            fixture.controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}
