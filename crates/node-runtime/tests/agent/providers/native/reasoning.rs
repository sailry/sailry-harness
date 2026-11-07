use super::*;

async fn requests_encrypted_reasoning_by_capability(fixture: &mut Fixture, case_name: &str) {
    for (known, catalog, effort, model, include) in [
        (Some(false), false, Effort::Default, "gpt-6-fixture", false),
        (None, false, Effort::Default, "gpt-4o", false),
        (None, false, Effort::High, "gpt-6-fixture", true),
        (None, false, Effort::High, "unknown-fixture", false),
        (
            Some(true),
            true,
            Effort::Disabled,
            "reasoning-fixture",
            true,
        ),
        (None, true, Effort::Disabled, "reasoning-fixture", true),
    ] {
        let server = Server::start(ModelApi::Responses, Reply::Text).await;
        fixture
            .select(
                "requests_encrypted_reasoning_by_capability",
                ModelApi::Responses,
                &server.endpoint,
            )
            .await;
        let mut provider = fixture.provider.clone();
        let model = model.to_owned();
        provider.models[0].id = model.clone();
        provider.default_model = model.clone();
        provider.models[0].reasoning = catalog;
        provider.models[0].capabilities = known.map(|reasoning| {
            serde_json::from_value(serde_json::json!({"reasoning":reasoning})).unwrap()
        });
        provider.models[0].efforts = vec![effort];
        provider.models[0].default_effort = effort;
        if known.is_none() && !catalog {
            // An automatically selected model has no advertised metadata.
            provider.models.clear();
        }
        let Output::Provider(_) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let mut config = fixture.session.config.clone();
        config.model = model;
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
        let turn = fixture.submit("Use the model capabilities").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{case_name}: {}",
            format_args!("known={known:?}, effort={effort:?}: {:?}", page.runs)
        );
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1, "{case_name}");
            let body = &requests[0].body;
            if include {
                assert_eq!(
                    body["include"],
                    serde_json::json!(["reasoning.encrypted_content"]),
                    "{case_name}"
                );
            } else {
                assert!(body.get("include").is_none(), "{case_name}");
            }
            assert!(body.get("previous_response_id").is_none(), "{case_name}");
        }
    }
}

async fn attempts_explicit_unknown_effort(fixture: &mut Fixture, case_name: &str) {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let server = crate::agent_support::Server::start(false).await;
        fixture
            .select("attempts_explicit_unknown_effort", api, &server.endpoint)
            .await;
        let mut provider = fixture.provider.clone();
        provider.models[0].reasoning = false;
        provider.models[0].efforts.clear();
        provider.models[0].default_effort = Effort::Default;
        provider.models[0].capabilities = None;
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        assert!(!provider.models[0].reasoning, "{case_name}");
        assert!(provider.models[0].efforts.is_empty(), "{case_name}");
        assert!(provider.models[0].capabilities.is_none(), "{case_name}");
        assert_eq!(fixture.session.config.effort, Effort::High, "{case_name}");
        let turn = fixture.submit("Use the selected effort").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{case_name}: {}",
            format_args!("{:?}", page.runs)
        );
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1, "{case_name}");
            assert_eq!(
                requests[0]["model"], fixture.session.config.model,
                "{case_name}"
            );
            let effort = if api == ModelApi::Responses {
                &requests[0]["reasoning"]["effort"]
            } else {
                &requests[0]["reasoning_effort"]
            };
            assert_eq!(*effort, "high", "{case_name}");
        }
    }
}

async fn accepts_unannotated_responses(fixture: &mut Fixture, case_name: &str) {
    // Compatible providers can omit annotations even when authenticated with an API key.
    let item = serde_json::json!({"type":"message", "id":"message-fixture", "role":"assistant", "status":"completed",
        "content":[{"type":"output_text", "text":"Compatible response"}]});
    let reasoning = serde_json::json!({"type":"reasoning", "id":"reasoning-fixture", "status":"completed",
        "summary":[{"type":"summary_text","text":"Inspect the requested files"}]});
    let events = [
        serde_json::json!({"type":"response.reasoning_summary_text.delta", "sequence_number":0, "item_id":"reasoning-fixture", "output_index":0, "summary_index":0, "delta":"Inspect the requested files"}),
        serde_json::json!({"type":"response.output_item.added", "sequence_number":1, "output_index":1, "item":item}),
        serde_json::json!({"type":"response.output_text.delta", "sequence_number":2, "item_id":"message-fixture", "output_index":1, "content_index":0, "delta":"Compatible response", "logprobs":[]}),
        serde_json::json!({"type":"response.completed", "sequence_number":3, "response":{
            "id":"response-fixture", "object":"response", "created_at":1, "status":"completed", "model":"fixture-a", "output":[reasoning,item]}}),
    ];
    let body = events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    let raw = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    let server = Server::start(
        ModelApi::Responses,
        Reply::Raw(Arc::new(raw.clone().into_bytes())),
    )
    .await;
    fixture
        .select(
            "accepts_unannotated_responses",
            ModelApi::Responses,
            &server.endpoint,
        )
        .await;
    let turn = fixture.submit("Compatible response fixture").await;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{case_name}: {}",
        format_args!("{:?}", page.runs)
    );
    assert!(text(&page).contains("Compatible response"), "{case_name}");
    assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::Thinking(text) if text.contains("Inspect the requested files"))), "{case_name}");
    {
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1, "{case_name}");
        assert_eq!(
            requests[0].headers["authorization"],
            format!("Bearer {}", server::KEY),
            "{case_name}"
        );
        assert_eq!(
            requests[0].body["reasoning"]["effort"], "high",
            "{case_name}"
        );
        assert_eq!(
            requests[0].body["reasoning"]["summary"], "auto",
            "{case_name}"
        );
    }
}

async fn preserves_openai_effort(fixture: &mut Fixture, case_name: &str) {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let server = crate::agent_support::Server::start(false).await;
        fixture
            .select("preserves_openai_effort", api, &server.endpoint)
            .await;
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
                "{case_name}: {}",
                format_args!("{:?}", page.runs)
            );
        }
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), choices.len(), "{case_name}");
        for (body, effort) in requests.iter().zip(choices) {
            let value = if api == ModelApi::Responses {
                &body["reasoning"]["effort"]
            } else {
                &body["reasoning_effort"]
            };
            let effective = if effort == Effort::Default {
                fixture.provider.models[0].default_effort
            } else {
                effort
            };
            let expected = match effective {
                Effort::Disabled => "none",
                Effort::Minimal => "minimal",
                Effort::Low => "low",
                Effort::Medium => "medium",
                Effort::High => "high",
                Effort::XHigh => "xhigh",
                Effort::Max => "max",
                _ => unreachable!(),
            };
            assert_eq!(
                *value,
                expected,
                "{case_name}: {}",
                format_args!("{api:?}, {effective:?}")
            );
        }
    }
}

#[tokio::test]
async fn restores_native_choices() {
    for (api, model, default, choices) in [
        (
            ModelApi::Anthropic,
            "fixture-a",
            Effort::High,
            vec![
                Effort::Default,
                Effort::Low,
                Effort::Medium,
                Effort::High,
                Effort::XHigh,
                Effort::Max,
                Effort::Budget(1024),
                Effort::Budget(4096),
            ],
        ),
        (
            ModelApi::Gemini,
            "gemini-3-flash-preview",
            Effort::High,
            vec![
                Effort::Default,
                Effort::Disabled,
                Effort::Low,
                Effort::Medium,
                Effort::High,
            ],
        ),
        (
            ModelApi::Gemini,
            "gemini-2.5-flash",
            Effort::Budget(2048),
            vec![
                Effort::Default,
                Effort::Disabled,
                Effort::Budget(2048),
                Effort::Budget(4096),
            ],
        ),
        (
            ModelApi::DeepSeek,
            "fixture-a",
            Effort::High,
            vec![Effort::Default, Effort::Disabled, Effort::High, Effort::Max],
        ),
    ] {
        let server = Server::start(api, Reply::Text).await;
        let mut fixture = Fixture::empty(api, &server.endpoint).await;
        fixture.provider.models[0].id = model.into();
        fixture.provider.default_model = model.into();
        fixture.provider.models[0].context = 16384;
        fixture.provider.models[0].output = 8192;
        fixture.provider.models[0].efforts = choices.clone();
        fixture.provider.models[0].default_effort = default;
        if api == ModelApi::Gemini {
            for effort in [Effort::Minimal, Effort::Budget(-1)] {
                let mut unsupported = fixture.provider.clone();
                unsupported.models[0].efforts = vec![effort];
                unsupported.models[0].default_effort = effort;
                let request = fixture.client.prepare(Command::PutProvider {
                    expected_revision: unsupported.revision,
                    provider: unsupported,
                });
                assert_eq!(
                    fixture.client.execute(request).await.unwrap_err().code,
                    ErrorCode::InvalidRequest
                );
            }
        }
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
            config.model = model.into();
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
        changed.models[0]
            .capabilities
            .get_or_insert_with(Default::default)
            .reasoning = Some(false);
        changed.models[0].efforts.clear();
        changed.models[0].default_effort = Effort::Default;
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
        let node = Node::start_empty(fixture.directory.path().join("node"))
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
        let client = Client::new(controller.handle().remote(address));
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
        for (request, &effort) in requests.iter().zip(&choices) {
            let effort = if effort == Effort::Default {
                fixture.provider.models[0].default_effort
            } else {
                effort
            };
            let body = &request.body;
            match api {
                ModelApi::Anthropic => {
                    assert_eq!(body["max_tokens"], 8192);
                    let budget = match effort {
                        Effort::Low => 4000,
                        Effort::Medium | Effort::High | Effort::XHigh | Effort::Max => 7168,
                        Effort::Budget(tokens) => tokens,
                        _ => unreachable!(),
                    };
                    assert_eq!(
                        body["thinking"],
                        serde_json::json!({"type":"enabled","budget_tokens":budget})
                    );
                    assert!(body.get("output_config").is_none());
                }
                ModelApi::Gemini => {
                    assert_eq!(
                        request.path,
                        format!("/v1beta/models/{model}:streamGenerateContent?alt=sse")
                    );
                    assert_eq!(body["generationConfig"]["maxOutputTokens"], 8192);
                    let expected = if model.starts_with("gemini-2.5") {
                        let (budget, include) = match effort {
                            Effort::Disabled => (0, false),
                            Effort::Budget(tokens) => (tokens, true),
                            _ => unreachable!(),
                        };
                        serde_json::json!({"thinkingBudget":budget,"includeThoughts":include})
                    } else {
                        match effort {
                            Effort::Disabled => serde_json::Value::Null,
                            Effort::Low => {
                                serde_json::json!({"thinkingLevel":"low","includeThoughts":true})
                            }
                            Effort::Medium => {
                                serde_json::json!({"thinkingLevel":"medium","includeThoughts":true})
                            }
                            Effort::High => {
                                serde_json::json!({"thinkingLevel":"high","includeThoughts":true})
                            }
                            _ => unreachable!(),
                        }
                    };
                    assert_eq!(
                        body["generationConfig"]["thinkingConfig"], expected,
                        "{model}, {effort:?}"
                    );
                }
                ModelApi::DeepSeek => {
                    assert_eq!(body["max_tokens"], 8192);
                    assert_eq!(
                        body["thinking"]["type"],
                        if effort == Effort::Disabled {
                            "disabled"
                        } else {
                            "enabled"
                        }
                    );
                    match effort {
                        Effort::Disabled => assert!(body.get("reasoning_effort").is_none()),
                        Effort::High => assert_eq!(body["reasoning_effort"], "high"),
                        Effort::Max => assert_eq!(body["reasoning_effort"], "max"),
                        _ => unreachable!(),
                    }
                }
                _ => unreachable!(),
            }
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

async fn preserves_streamed_whitespace(fixture: &mut Fixture, case_name: &str) {
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
    for api in [ModelApi::DeepSeek, ModelApi::ChatCompletions] {
        let body = body.clone();
        let server = Server::start(
                api,
                Reply::Dynamic(Arc::new(move |index| {
                    let body = body.replace(
                        "\"id\":\"fixture\"",
                        &format!("\"id\":\"fixture-{index}\""),
                    );
                    server::Script {
                        bytes: format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .into_bytes(),
                        ending: server::Ending::Close,
                    }
                })),
            )
            .await;
        fixture
            .select("preserves_streamed_whitespace", api, &server.endpoint)
            .await;
        for prompt in ["First", "Continue"] {
            let turn = fixture.submit(prompt).await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{case_name}"
            );
            assert!(
                page.entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| { matches!(part, Part::Thinking(value) if value == thinking) }),
                "{case_name}"
            );
        }
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2, "{case_name}");
        let assistant = requests[1].body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "assistant")
            .unwrap();
        assert_eq!(
            assistant["content"],
            answer,
            "{case_name}: {}",
            format_args!("{api:?}")
        );
        assert_eq!(assistant["reasoning_content"], thinking, "{case_name}");
        assert!(assistant.get("reasoning").is_none(), "{case_name}");
    }
}

#[tokio::test]
async fn preserves_inline_thinking() {
    let thinking = "Consider the greeting";
    let answer = "你好！有什么我可以帮你？";
    let prompts = ["Say hello", "Continue the greeting"];
    let server = Server::start(
        ModelApi::ChatCompletions,
        Reply::Dynamic(Arc::new(move |index| {
            let id = format!("inline-thinking-{index}");
            let events = [
                serde_json::json!({
                    "id":id, "object":"chat.completion.chunk", "created":1, "model":"fixture-a",
                    "choices":[{"index":0,"delta":{"content":format!("<think>{thinking}</think>{answer}")},"finish_reason":null}]
                }),
                serde_json::json!({
                    "id":id, "object":"chat.completion.chunk", "created":1, "model":"fixture-a",
                    "choices":[{"index":0,"delta":{},"finish_reason":"stop"}]
                }),
            ];
            let mut body = events
                .into_iter()
                .map(|event| format!("data: {event}\n\n"))
                .collect::<String>();
            body.push_str("data: [DONE]\n\n");
            server::Script {
                bytes: format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .into_bytes(),
                ending: server::Ending::Close,
            }
        })),
    )
    .await;
    let fixture = Fixture::empty(ModelApi::ChatCompletions, &server.endpoint).await;
    let expected = vec![Part::Text(answer.into()), Part::Thinking(thinking.into())];
    let mut turns = Vec::new();
    for prompt in prompts {
        let turn = fixture.submit(prompt).await;
        turns.push(turn);
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.iter().find(|run| run.turn == turn).unwrap();
        assert_eq!(run.status, Status::Completed, "{run:?}");
        assert!(run.error.is_none(), "{run:?}");
        for &recorded in &turns {
            let entries = page
                .entries
                .iter()
                .filter(|entry| entry.turn == recorded && entry.author == "assistant")
                .collect::<Vec<_>>();
            assert_eq!(entries.len(), 1, "turn={recorded}");
            assert_eq!(entries[0].parts, expected, "turn={recorded}");
        }
    }
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.path, "/v1/chat/completions");
        assert_eq!(request.body["stream"], true);
    }
    let messages = requests[1].body["messages"].as_array().unwrap();
    let assistants = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message["role"] == "assistant")
        .collect::<Vec<_>>();
    assert_eq!(assistants.len(), 1);
    let (position, assistant) = assistants[0];
    assert_eq!(assistant["content"], answer);
    assert_eq!(assistant["reasoning_content"], thinking);
    let [first, follow_up] = prompts.map(|prompt| {
        messages
            .iter()
            .position(|message| {
                message["role"] == "user" && message["content"].to_string().contains(prompt)
            })
            .unwrap_or_else(|| panic!("user prompt missing: {prompt}"))
    });
    assert!(first < position && position < follow_up);
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn choices() {
    use futures::FutureExt;
    let mut fixture = Fixture::empty(ModelApi::Responses, "http://127.0.0.1:9").await;
    std::panic::AssertUnwindSafe(Box::pin(requests_encrypted_reasoning_by_capability(
        &mut fixture,
        "requests_encrypted_reasoning_by_capability",
    )))
    .catch_unwind()
    .await
    .unwrap_or_else(|error| {
        eprintln!("step: requests_encrypted_reasoning_by_capability");
        std::panic::resume_unwind(error)
    });
    std::panic::AssertUnwindSafe(Box::pin(attempts_explicit_unknown_effort(
        &mut fixture,
        "attempts_explicit_unknown_effort",
    )))
    .catch_unwind()
    .await
    .unwrap_or_else(|error| {
        eprintln!("step: attempts_explicit_unknown_effort");
        std::panic::resume_unwind(error)
    });
    std::panic::AssertUnwindSafe(Box::pin(accepts_unannotated_responses(
        &mut fixture,
        "accepts_unannotated_responses",
    )))
    .catch_unwind()
    .await
    .unwrap_or_else(|error| {
        eprintln!("step: accepts_unannotated_responses");
        std::panic::resume_unwind(error)
    });
    std::panic::AssertUnwindSafe(Box::pin(preserves_openai_effort(
        &mut fixture,
        "preserves_openai_effort",
    )))
    .catch_unwind()
    .await
    .unwrap_or_else(|error| {
        eprintln!("step: preserves_openai_effort");
        std::panic::resume_unwind(error)
    });
    std::panic::AssertUnwindSafe(Box::pin(preserves_streamed_whitespace(
        &mut fixture,
        "preserves_streamed_whitespace",
    )))
    .catch_unwind()
    .await
    .unwrap_or_else(|error| {
        eprintln!("step: preserves_streamed_whitespace");
        std::panic::resume_unwind(error)
    });
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}
