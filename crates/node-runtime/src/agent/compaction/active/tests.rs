use super::*;

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
fn indexes_rebuilt_anonymous_results() {
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
    let prior = events.clone();
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
    let mut stale = request.clone();
    let stale_revisions = model::history::revisions(&prior);
    assert!(model::context_overflow(
        &model::history::prepare(&mut stale, Some(&stale_revisions))
            .limit(&mut stale, Some(window))
            .unwrap_err()
    ));

    let revisions = model::history::revisions(&history.events);
    let mut outputs = model::history::prepare(&mut request, Some(&revisions));
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
    assert!(id.is_none());
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
