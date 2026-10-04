use super::*;

#[tokio::test]
async fn replays_selected_field() {
    use adk_core::{Content, FunctionResponseData, Llm, LlmRequest, Part as AdkPart};
    use adk_model::{OpenAICompatible, OpenAICompatibleConfig, ReasoningReplayField};
    use futures::StreamExt;
    for field in [
        None,
        Some(ReasoningReplayField::ReasoningContent),
        Some(ReasoningReplayField::Reasoning),
    ] {
        let server = Server::start(ModelApi::ChatCompletions, Reply::Text).await;
        let mut model = OpenAICompatible::new(
            OpenAICompatibleConfig::new(server::KEY, "fixture-a").with_base_url(&server.endpoint),
        )
        .unwrap();
        if let Some(field) = field {
            model = model.with_reasoning_replay_field(field);
        }
        let mut assistant = Content::new("assistant").with_thinking("Inspect the file");
        assistant.parts.push(AdkPart::FunctionCall {
            name: "read_file".into(),
            args: serde_json::json!({"path":"native.txt"}),
            id: Some("call-1".into()),
            thought_signature: None,
        });
        let mut tool = Content::new("tool");
        tool.parts.push(AdkPart::FunctionResponse {
            function_response: FunctionResponseData::new(
                "read_file",
                serde_json::json!({"text":"file content"}),
            ),
            id: Some("call-1".into()),
            annotations: None,
        });
        let mut stream = model
            .generate_content(
                LlmRequest::new(
                    "fixture-a",
                    vec![
                        Content::new("user").with_text("Read the file"),
                        assistant,
                        tool,
                    ],
                ),
                true,
            )
            .await
            .unwrap();
        while let Some(response) = stream.next().await {
            response.unwrap();
        }
        let requests = server.requests.lock().unwrap();
        let messages = requests[0].body["messages"].as_array().unwrap();
        let assistant = &messages[1];
        assert!(assistant["content"].is_null());
        assert_eq!(assistant["tool_calls"][0]["id"], "call-1");
        assert_eq!(messages[2]["tool_call_id"], "call-1");
        for (name, selected) in [
            (
                "reasoning_content",
                Some(ReasoningReplayField::ReasoningContent),
            ),
            ("reasoning", Some(ReasoningReplayField::Reasoning)),
        ] {
            if field == selected {
                assert_eq!(assistant[name], "Inspect the file");
            } else {
                assert!(assistant.get(name).is_none());
            }
        }
    }
}

#[tokio::test]
async fn accepts_unannotated_responses() {
    // Compatible providers can omit annotations even when authenticated with an API key.
    let item = serde_json::json!({"type":"message", "id":"message-fixture", "role":"assistant", "status":"completed",
        "content":[{"type":"output_text", "text":"Compatible response"}]});
    let events = [
        serde_json::json!({"type":"response.reasoning_summary_text.delta", "sequence_number":0, "item_id":"reasoning-fixture", "output_index":0, "summary_index":0, "delta":"Inspect the requested files"}),
        serde_json::json!({"type":"response.output_item.added", "sequence_number":0, "output_index":0, "item":item}),
        serde_json::json!({"type":"response.output_text.delta", "sequence_number":1, "item_id":"message-fixture", "output_index":0, "content_index":0, "delta":"Compatible response", "logprobs":[]}),
        serde_json::json!({"type":"response.completed", "sequence_number":2, "response":{
            "id":"response-fixture", "object":"response", "created_at":1, "status":"completed", "model":"fixture-a", "output":[item]}}),
    ];
    let body = events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    let raw = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    for remote in [false, true] {
        let server = Server::start(
            ModelApi::Responses,
            Reply::Raw(Arc::new(raw.clone().into_bytes())),
        )
        .await;
        let fixture = Fixture::new(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Compatible response fixture").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert!(text(&page).contains("Compatible response"));
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::Thinking(text) if text.contains("Inspect the requested files"))));
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(
                requests[0].headers["authorization"],
                format!("Bearer {}", server::KEY)
            );
            assert_eq!(requests[0].body["reasoning"]["effort"], "high");
            assert_eq!(requests[0].body["reasoning"]["summary"], "auto");
        }
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn preserves_openai_effort() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        for remote in [false, true] {
            let server = crate::agent_support::Server::start(false).await;
            let mut fixture = Fixture::new(remote, api, &server.endpoint).await;
            let choices = vec![
                Effort::Default,
                Effort::Disabled,
                Effort::Minimal,
                Effort::Low,
                Effort::Medium,
                Effort::High,
                Effort::XHigh,
                Effort::Max,
            ];
            fixture.provider.models[0].efforts = choices.clone();
            execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: fixture.provider.revision,
                    provider: fixture.provider.clone(),
                },
            )
            .await;
            for effort in choices.iter().copied() {
                let mut config = fixture.session.config.clone();
                config.effort = effort;
                let Output::Session(session) = execute(
                    &fixture.client,
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
                let turn = fixture.submit("OpenAI reasoning fixture").await;
                let page = finished(&fixture.client, fixture.session.id, turn).await;
                assert_eq!(
                    page.runs
                        .iter()
                        .find(|run| run.turn == turn)
                        .unwrap()
                        .status,
                    Status::Completed,
                    "{:?}",
                    page.runs
                );
            }
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), choices.len());
            for (body, effort) in requests.iter().zip(choices) {
                let value = if api == ModelApi::Responses {
                    &body["reasoning"]["effort"]
                } else {
                    &body["reasoning_effort"]
                };
                if effort == Effort::Default {
                    assert!(value.is_null());
                } else {
                    assert_eq!(*value, serde_json::to_value(effort).unwrap());
                }
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn restores_native_choices() {
    for api in [ModelApi::Anthropic, ModelApi::Gemini, ModelApi::DeepSeek] {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Text).await;
            let mut fixture = Fixture::new(remote, api, &server.endpoint).await;
            let choices = match api {
                ModelApi::Anthropic => vec![
                    Effort::Default,
                    Effort::Low,
                    Effort::Medium,
                    Effort::High,
                    Effort::XHigh,
                    Effort::Max,
                    Effort::Budget(1024),
                    Effort::Budget(4096),
                ],
                ModelApi::Gemini => vec![
                    Effort::Default,
                    Effort::Disabled,
                    Effort::Minimal,
                    Effort::Low,
                    Effort::Medium,
                    Effort::High,
                    Effort::Budget(-1),
                    Effort::Budget(2048),
                ],
                ModelApi::DeepSeek => {
                    vec![Effort::Default, Effort::Disabled, Effort::High, Effort::Max]
                }
                _ => unreachable!(),
            };
            fixture.provider.models[0].context = 16384;
            fixture.provider.models[0].output = 8192;
            fixture.provider.models[0].efforts = choices.clone();
            let Output::Provider(provider) = execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: fixture.provider.revision,
                    provider: fixture.provider.clone(),
                },
            )
            .await
            else {
                panic!("provider expected")
            };
            let mut turns = Vec::new();
            for &effort in &choices {
                let mut config = fixture.session.config.clone();
                config.effort = effort;
                let Output::Session(session) = execute(
                    &fixture.client,
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
                let Output::QueuedTurn(turn) = execute(
                    &fixture.client,
                    Command::QueueTurn {
                        session: fixture.session.id,
                        expected_revision: fixture.session.revision,
                        message: "reasoning fixture".into(),
                    },
                )
                .await
                else {
                    panic!("turn expected")
                };
                assert_eq!(turn.config.effort, effort);
                turns.push(turn);
            }
            assert!(server.requests.lock().unwrap().is_empty());
            let mut changed = provider;
            changed.endpoint = "http://127.0.0.1:9/changed".into();
            changed.models[0].reasoning = false;
            changed.models[0].efforts.clear();
            execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: changed.revision,
                    provider: changed,
                },
            )
            .await;
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
            let node = Node::start(fixture.directory.path().join("node"))
                .await
                .unwrap();
            let controller = Link::controller(
                fixture.directory.path().join("resumed"),
                NetworkScope::default(),
            )
            .await
            .unwrap();
            let invitation = node.link().invite().unwrap();
            let address = controller.handle().pair(invitation.ticket()).await.unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(address)
            } else {
                node.local()
            });
            for turn in turns {
                execute(&client, Command::StartQueuedTurn { turn: turn.id }).await;
                let page = finished(&client, turn.session, turn.id).await;
                assert_eq!(
                    page.runs
                        .iter()
                        .find(|run| run.turn == turn.id)
                        .unwrap()
                        .status,
                    Status::Completed,
                    "{:?}",
                    page.runs
                );
            }
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), choices.len());
            for (request, effort) in requests.iter().zip(choices) {
                let body = &request.body;
                match api {
                    ModelApi::Anthropic => {
                        assert_eq!(body["max_tokens"], 8192);
                        match effort {
                            Effort::Default => {
                                assert!(body.get("thinking").is_none());
                                assert!(body.get("output_config").is_none());
                            }
                            Effort::Budget(tokens) => {
                                assert_eq!(
                                    body["thinking"],
                                    serde_json::json!({"type":"enabled","budget_tokens":tokens})
                                );
                                assert!(body.get("output_config").is_none());
                            }
                            effort => {
                                assert_eq!(body["thinking"]["type"], "adaptive");
                                assert_eq!(
                                    body["output_config"]["effort"],
                                    serde_json::to_value(effort).unwrap()
                                );
                            }
                        }
                    }
                    ModelApi::Gemini => {
                        let generation = &body["generationConfig"];
                        assert_eq!(generation["maxOutputTokens"], 8192);
                        let thinking = &generation["thinkingConfig"];
                        match effort {
                            Effort::Default => assert!(generation.get("thinkingConfig").is_none()),
                            Effort::Disabled => {
                                assert_eq!(thinking["thinkingBudget"], 0);
                                assert!(thinking.get("thinkingLevel").is_none());
                                assert!(thinking.get("includeThoughts").is_none());
                            }
                            Effort::Budget(tokens) => {
                                assert_eq!(thinking["thinkingBudget"], tokens);
                                assert!(thinking.get("thinkingLevel").is_none());
                                assert_eq!(thinking["includeThoughts"], true);
                            }
                            effort => {
                                assert_eq!(
                                    thinking["thinkingLevel"],
                                    serde_json::to_value(effort).unwrap()
                                );
                                assert!(thinking.get("thinkingBudget").is_none());
                            }
                        }
                    }
                    ModelApi::DeepSeek => {
                        assert_eq!(body["max_tokens"], 8192);
                        match effort {
                            Effort::Default => assert!(body.get("thinking").is_none()),
                            Effort::Disabled => assert_eq!(body["thinking"]["type"], "disabled"),
                            _ => {
                                assert_eq!(body["thinking"]["type"], "enabled");
                                assert_eq!(
                                    body["reasoning_effort"],
                                    serde_json::to_value(effort).unwrap()
                                );
                            }
                        }
                    }
                    _ => unreachable!(),
                }
            }
            controller.close().await.unwrap();
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn preserves_streamed_whitespace() {
    let answer = "中文 paragraph\n\n`https://www.example.com`\n\n- next";
    let thinking = "Inspect the question before answering";
    let mut body = answer.chars().map(|ch| format!("data: {}\n\n", serde_json::json!({
        "id":"fixture", "object":"chat.completion.chunk", "created":1, "model":"fixture-a",
        "choices":[{"index":0,"delta":{"content":ch.to_string()},"finish_reason":null}]
    }))).collect::<String>();
    body.insert_str(
        0,
        &format!(
            "data: {}\n\n",
            serde_json::json!({
                "id":"fixture", "object":"chat.completion.chunk", "created":1, "model":"fixture-a",
                "choices":[{"index":0,"delta":{"reasoning_content":thinking},"finish_reason":null}]
            })
        ),
    );
    body.push_str(&format!(
        "data: {}\n\ndata: [DONE]\n\n",
        serde_json::json!({
            "id":"fixture", "object":"chat.completion.chunk", "created":1, "model":"fixture-a",
            "choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":20,"completion_tokens":20,"total_tokens":40}
        })
    ));
    let raw = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    for api in [ModelApi::DeepSeek, ModelApi::ChatCompletions] {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Raw(Arc::new(raw.clone().into_bytes()))).await;
            let fixture = Fixture::new(remote, api, &server.endpoint).await;
            for prompt in ["First", "Continue"] {
                let turn = fixture.submit(prompt).await;
                let page = finished(&fixture.client, fixture.session.id, turn).await;
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                assert!(
                    page.entries
                        .iter()
                        .flat_map(|entry| &entry.parts)
                        .any(|part| { matches!(part, Part::Thinking(value) if value == thinking) })
                );
            }
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 2);
            let assistant = requests[1].body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["role"] == "assistant")
                .unwrap();
            assert_eq!(assistant["content"], answer, "{api:?}, remote={remote}");
            if api == ModelApi::DeepSeek {
                assert_eq!(assistant["reasoning_content"], thinking);
            } else {
                assert!(assistant.get("reasoning_content").is_none());
                assert!(assistant.get("reasoning").is_none());
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
