use super::*;

#[tokio::test]
async fn omits_encrypted_replay() {
    for remote in [false, true] {
        for (known, catalog, effort, include) in [
            (Some(false), false, Effort::Default, false),
            (None, false, Effort::Default, false),
            (None, false, Effort::High, true),
            (Some(true), true, Effort::Disabled, true),
            (None, true, Effort::Disabled, true),
        ] {
            let server = Server::start(ModelApi::Responses, Reply::Text).await;
            let mut current = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
            let mut provider = current.provider.clone();
            let model = if include {
                "reasoning-fixture"
            } else {
                "gpt-4o"
            }
            .to_owned();
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
                &current.client,
                Command::PutProvider {
                    expected_revision: provider.revision,
                    provider,
                },
            )
            .await
            else {
                panic!("provider expected")
            };
            let mut config = current.session.config.clone();
            config.model = model;
            config.effort = effort;
            let Output::Session(session) = execute(
                &current.client,
                Command::SetSessionConfig {
                    session: current.session.id,
                    expected_revision: current.session.revision,
                    config,
                },
            )
            .await
            else {
                panic!("session expected")
            };
            current.session = session;
            let turn = current.submit("Use the model capabilities").await;
            let page = finished(&current.client, current.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "remote={remote}, known={known:?}, effort={effort:?}: {:?}",
                page.runs
            );
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), 1);
                let body = &requests[0].body;
                assert!(body.get("include").is_none());
                assert!(body.get("previous_response_id").is_none());
            }
            current.controller.close().await.unwrap();
            current.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn attempts_explicit_unknown_effort() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        for remote in [false, true] {
            let server = crate::agent_support::Server::start(false).await;
            let fixture = Fixture::empty(remote, api, &server.endpoint).await;
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
            assert!(!provider.models[0].reasoning);
            assert!(provider.models[0].efforts.is_empty());
            assert!(provider.models[0].capabilities.is_none());
            assert_eq!(fixture.session.config.effort, Effort::High);
            let turn = fixture.submit("Use the selected effort").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), 1);
                assert_eq!(requests[0]["model"], fixture.session.config.model);
                let effort = if api == ModelApi::Responses {
                    &requests[0]["reasoning"]["effort"]
                } else {
                    &requests[0]["reasoning_effort"]
                };
                assert!(effort.is_null());
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn accepts_unannotated_responses() {
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
    for remote in [false, true] {
        let server = Server::start(
            ModelApi::Responses,
            Reply::Raw(Arc::new(raw.clone().into_bytes())),
        )
        .await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
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
            let mut fixture = Fixture::empty(remote, api, &server.endpoint).await;
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
                let effective = if effort == Effort::Default {
                    fixture.provider.models[0].default_effort
                } else {
                    effort
                };
                let expected = match effective {
                    Effort::Disabled | Effort::Minimal | Effort::Low => "low",
                    Effort::Medium => "medium",
                    Effort::High | Effort::XHigh | Effort::Max => "high",
                    _ => unreachable!(),
                };
                assert_eq!(*value, expected, "{api:?}, {effective:?}");
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
            let mut fixture = Fixture::empty(remote, api, &server.endpoint).await;
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
            let client = Client::new(if remote {
                controller.handle().remote(address)
            } else {
                node.local()
            });
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
            for (request, effort) in requests.iter().zip(choices) {
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
                        assert_eq!(body["generationConfig"]["maxOutputTokens"], 8192);
                        assert!(body["generationConfig"].get("thinkingConfig").is_none());
                    }
                    ModelApi::DeepSeek => {
                        assert_eq!(body["max_tokens"], 8192);
                        assert!(body.get("thinking").is_none());
                        assert_eq!(
                            body["reasoning_effort"],
                            if effort == Effort::Disabled {
                                "low"
                            } else {
                                "high"
                            }
                        );
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
    for api in [ModelApi::DeepSeek, ModelApi::ChatCompletions] {
        for remote in [false, true] {
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
            let fixture = Fixture::empty(remote, api, &server.endpoint).await;
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
            assert_eq!(assistant["reasoning_content"], thinking);
            assert!(assistant.get("reasoning").is_none());
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
