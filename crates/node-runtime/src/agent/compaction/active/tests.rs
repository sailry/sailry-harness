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
