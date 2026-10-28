use super::*;

#[tokio::test]
async fn prepares_each_ordinary_request_once() {
    use adk_session::{CreateRequest, InMemorySessionService};
    use sailry_client::Client;
    use sailry_protocol::{Command, Output, Permission, ProviderId, SessionConfig, WorkMode};

    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let client = Client::new(node.local());
    let Output::Session(session) = client
        .execute(client.prepare(Command::CreateSession {
            project: None,
            worktree: None,
            config: Some(SessionConfig {
                provider: ProviderId::new(),
                model: "summary-fixture".into(),
                credential: None,
                assistant: None,
                resource: None,
                effort: Default::default(),
                features: Default::default(),
                options: Default::default(),
                mode: WorkMode::Code,
                permission: Permission::Ask,
            }),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    let service = Arc::new(InMemorySessionService::new());
    service
        .create(CreateRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: Some(session.id.to_string()),
            state: Default::default(),
        })
        .await
        .unwrap();
    let (mut summary, _) = super::super::tests::summarizer(vec![], false);
    summary.context = Some(128_000);
    let context = Context::new(node.ingress.clone(), service, session.id, summary);
    let output = json!({"text":"Canonical ordinary evidence ".repeat(300)});
    let request = LlmRequest::new(
        "summary-fixture",
        vec![
            context.preamble().clone(),
            Content {
                role: "model".into(),
                parts: vec![Part::FunctionCall {
                    id: Some("adk-call-fixture".into()),
                    name: "mcp_fixture".into(),
                    args: json!({"path":"fixture"}),
                    thought_signature: None,
                }],
            },
            Content {
                role: "function".into(),
                parts: vec![Part::FunctionResponse {
                    id: Some("adk-call-fixture".into()),
                    function_response: adk_core::FunctionResponseData::new(
                        "mcp_fixture",
                        json!({"output":output,"text":[output.to_string()]}),
                    ),
                    annotations: None,
                }],
            },
        ],
    );
    let preparations = model::history::PREPARATIONS.get();
    let request = context
        .transform("current", request, &[], false)
        .await
        .unwrap();
    assert_eq!(model::history::PREPARATIONS.get(), preparations + 1);
    let stream = model::history::generate(
        request,
        adk_core::ProviderOrigin {
            provider: "gemini".into(),
            model: "summary-fixture".into(),
            scope: "fixture".into(),
        },
        context.window(),
        |request| async move {
            assert_eq!(model::history::PREPARATIONS.get(), preparations + 1);
            let Part::FunctionResponse {
                id,
                function_response,
                ..
            } = &request.contents[2].parts[0]
            else {
                panic!("result expected")
            };
            assert!(id.is_none());
            assert_eq!(function_response.response["output"], output);
            assert!(function_response.response.get("text").is_none());
            assert!(function_response.response.get("sailry_output").is_none());
            Ok(Box::pin(futures::stream::empty()) as adk_core::LlmResponseStream)
        },
    )
    .await
    .unwrap();
    drop(stream);
    node.shutdown().await.unwrap();
}

#[test]
fn preserves_retained_provider_parts() {
    let preamble = Content::new("system").with_text("Instructions");
    let content = Content::new("model").with_text("Repeated response");
    let mut event = Event::new("invocation");
    let mut augmented = content.clone();
    augmented.parts.push(Part::Thinking {
        thinking: "Retained reasoning".into(),
        signature: Some("retained-signature".into()),
    });
    event.set_content(augmented.clone());
    let mut request = LlmRequest {
        model: "fixture".into(),
        contents: vec![
            preamble.clone().with_text("Scoped project instructions"),
            content,
            augmented.clone(),
        ],
        config: None,
        tools: Default::default(),
        previous_response_id: None,
    };
    let index = tail(&request, &[event.clone()], &preamble, "current").unwrap();
    assert_eq!(index, 2);
    request
        .contents
        .splice(1..index, [Content::new("user").with_text("Summary")]);
    assert_eq!(request.contents[2].parts, augmented.parts);
    assert!(
        tail(
            &request,
            &[event.clone()],
            &Content::new("system").with_text("Changed instructions"),
            "current",
        )
        .is_none()
    );
    request.contents[2].parts.reverse();
    assert!(tail(&request, &[event], &preamble, "current").is_none());
}

#[test]
fn projects_rebuilt_results_without_history_index() {
    let preamble = Content::new("system").with_text("Captured instructions");
    let mut summary = Event::new("summary");
    summary.author = "user".into();
    summary.set_content(Content::new("user").with_text("Saved handoff"));
    let result = |label: &str, value: serde_json::Value| {
        let mut call = Event::new("current");
        call.author = "assistant".into();
        call.set_content(Content {
            role: "model".into(),
            parts: vec![Part::FunctionCall {
                id: Some(format!("adk-call-{label}")),
                name: "mcp_fixture".into(),
                args: json!({"path":label}),
                thought_signature: None,
            }],
        });
        let mut response = Event::new("current");
        response.author = "assistant".into();
        response.set_content(Content {
            role: "function".into(),
            parts: vec![Part::FunctionResponse {
                id: Some(format!("adk-call-{label}")),
                function_response: adk_core::FunctionResponseData::new("mcp_fixture", value),
                annotations: None,
            }],
        });
        [call, response]
    };
    let mut events = vec![summary];
    events.extend(result("old", json!({"text":"Retained evidence"})));
    let original = json!({"text":"Result arriving during the summary 中文 🙂 ".repeat(4000)});
    events.extend(result("late", original.clone()));
    let canonical = serde_json::to_vec(&events).unwrap();
    let history = History {
        events,
        completed: vec![],
        current: 1,
        recent: None,
    };
    let mut request = LlmRequest::new(
        "fixture",
        vec![
            preamble.clone(),
            Content::new("user").with_text("Old context"),
        ],
    );
    matching::rebuild(&mut request, &history.events, &preamble, "current").unwrap();
    let window = model::history::Window {
        context: 4_000,
        output: 500,
    };
    let mut outputs = model::history::prepare(&mut request, None);
    assert!(outputs.mask(&mut request, 0, 1).unwrap() > 0);
    outputs.limit(&mut request, Some(window)).unwrap();
    let Part::FunctionResponse {
        id,
        function_response,
        ..
    } = request.contents.last().unwrap().parts.first().unwrap()
    else {
        panic!("late result expected")
    };
    assert_eq!(id.as_deref(), Some("adk-call-late"));
    assert_eq!(function_response.response["masked"], true);
    assert_eq!(
        function_response.response["output"]["call"],
        "adk-call-late"
    );
    let revision = blake3::hash(original.to_string().as_bytes())
        .to_hex()
        .to_string();
    assert_eq!(function_response.response["output"]["revision"], revision);
    assert_eq!(
        model::history::masked(&request),
        std::collections::BTreeSet::from([("adk-call-late".into(), revision)])
    );
    assert!(outputs.tokens(&request).unwrap() <= window.context - window.output);
    assert_eq!(serde_json::to_vec(&history.events).unwrap(), canonical);
}

#[tokio::test]
async fn selects_complete_projected_prefix() {
    use adk_session::InMemorySessionService;

    let event = |author: &str, parts| {
        let mut event = Event::new("previous");
        event.author = author.into();
        event.set_content(Content {
            role: author.into(),
            parts,
        });
        event
    };
    let call = |id: &str, name: &str, args| Part::FunctionCall {
        id: Some(id.into()),
        name: name.into(),
        args,
        thought_signature: None,
    };
    let result = |id: &str, name: &str, value| Part::FunctionResponse {
        id: Some(id.into()),
        function_response: adk_core::FunctionResponseData::new(name, value),
        annotations: None,
    };
    let mut input = event(
        "user",
        vec![Part::text_part(
            "Preserve checkpoint-fixture and the original command reference. ".repeat(32),
        )],
    );
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_SOURCE.into(), "user".into());
    input
        .provider_metadata
        .insert(crate::store::agent::INPUT_PART.into(), "0".into());
    let file = |path: &str| {
        json!({"kind":"file_lines","data":{
            "path":path,"revision":"a".repeat(64),"size":520,"offset":0,
            "lines":(1..=40).map(|number| json!({"number":number,"text":"Source facts\n"})).collect::<Vec<_>>(),"next_offset":null
        }})
    };
    let mut history = History {
        events: vec![
            input,
            event(
                "model",
                vec![call(
                    "title",
                    "set_session_title",
                    json!({"title":"Fixture"}),
                )],
            ),
            event(
                "function",
                vec![result(
                    "title",
                    "set_session_title",
                    json!({"title":"Fixture"}),
                )],
            ),
            event(
                "model",
                vec![
                    call("left", "read_file", json!({"path":"left.txt"})),
                    call("right", "read_file", json!({"path":"right.txt"})),
                    call("command", "run_command", json!({"command":"fixture"})),
                ],
            ),
            event(
                "function",
                vec![
                    result("left", "read_file", file("left.txt")),
                    result("right", "read_file", file("right.txt")),
                    result(
                        "command",
                        "run_command",
                        json!({"kind":"command_result","data":{
                            "stdout":{"text":"Command evidence\n".repeat(65_536),"truncated":false,"invalid_utf8":false},
                            "stderr":{"text":"","truncated":false,"invalid_utf8":false},
                            "outcome":{"kind":"exited","data":0},"elapsed_ms":1
                        }}),
                    ),
                ],
            ),
            event(
                "model",
                vec![call(
                    "write",
                    "write_file",
                    json!({"path":"done.txt","content":"Verified"}),
                )],
            ),
            event(
                "function",
                vec![result(
                    "write",
                    "write_file",
                    json!({"kind":"file_written","data":{"path":"done.txt","revision":"a".repeat(64),"size":8}}),
                )],
            ),
            event("model", vec![Part::text_part("DONE")]),
        ],
        completed: vec![3, 5, 7, 8],
        current: 8,
        recent: None,
    };
    let canonical = serde_json::to_vec(&history.events).unwrap();
    let response =
        super::super::tests::complete("Keep the checkpoint and original command reference.");
    let (mut small, rejected) = super::super::tests::summarizer(vec![response.clone()], false);
    small.context = Some(64_000);
    small.output = Some(8_192);
    // The former global call limit selected only the input and title exchange.
    // Reattaching the exact instruction makes even this concise response grow.
    let error = small
        .summarize_with(&history.events[..3], SummaryMode::Automatic)
        .await
        .unwrap_err();
    assert_eq!(
        failure::classify(&error),
        (Stage::Summary, Reason::NoReduction)
    );
    assert_eq!(rejected.requests.lock().unwrap().len(), 1);

    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let (mut summarizer, model) = super::super::tests::summarizer(vec![response], false);
    summarizer.context = Some(64_000);
    summarizer.output = Some(8_192);
    let context = Context::new(
        node.ingress.clone(),
        Arc::new(InMemorySessionService::new()),
        sailry_protocol::SessionId::new(),
        summarizer,
    );
    let prepared = context.summarizer.prepare(&history.events).unwrap();
    let raw_tail = estimate::tokens(&history.events[3..]);
    let projected_tail = prepared
        .request
        .contents
        .iter()
        .skip(4)
        .take(5)
        .map(|content| 4 + content.parts.iter().map(estimate::part).sum::<u64>())
        .sum::<u64>();
    assert!(raw_tail > 262_144);
    assert!(projected_tail < context.summarizer.recent());
    let candidates = context.candidates(&history, 1);
    assert_eq!(candidates.first(), Some(&history.events.len()));
    let cut = context
        .summarizer
        .prefix(
            &history.events,
            candidates.into_iter(),
            context.summarizer.recent(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(cut, history.events.len());
    assert!(history.completed.contains(&cut));
    let summary = context
        .summarizer
        .summarize_with(&history.events[..cut], SummaryMode::Automatic)
        .await
        .unwrap()
        .unwrap();
    assert!(summary.actions.compaction.is_some());
    assert_eq!(model.requests.lock().unwrap().len(), 1);
    assert_eq!(serde_json::to_vec(&history.events).unwrap(), canonical);
    println!("projected prefix: raw_tail={raw_tail} projected_tail={projected_tail} cut={cut}");

    // Two recent text-only turns protect their own boundaries. The current
    // incomplete parallel batch must also remain entirely after the summary.
    let old_end = history.events.len();
    history.events.extend([
        event("user", vec![Part::text_part("Recent B")]),
        event("model", vec![Part::text_part("Verified B")]),
        event("user", vec![Part::text_part("Recent C")]),
        event("model", vec![Part::text_part("Verified C")]),
        event("user", vec![Part::text_part("Compact prior work")]),
        event(
            "model",
            vec![
                call("compact", "compact_context", json!({})),
                call("pending", "read_file", json!({"path":"pending.txt"})),
            ],
        ),
        event(
            "function",
            vec![result("pending", "read_file", file("pending.txt"))],
        ),
    ]);
    history.completed.extend([old_end + 2, old_end + 4]);
    history.current = old_end + 4;
    history.recent = Some(old_end);
    let canonical = serde_json::to_vec(&history.events).unwrap();
    let candidates = context.candidates(&history, 2);
    assert_eq!(candidates.first(), Some(&old_end));
    assert!(candidates.iter().all(|cut| *cut <= old_end));
    let cut = context
        .summarizer
        .prefix(
            &history.events,
            candidates.into_iter(),
            context.summarizer.recent(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(cut, old_end);
    assert_eq!(serde_json::to_vec(&history.events).unwrap(), canonical);
    node.shutdown().await.unwrap();
}
