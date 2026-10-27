use super::*;

#[test]
fn advertises_model_output_capacity() {
    let (mut summarizer, _) = summarizer(vec![], false);
    summarizer.context = Some(1_050_000);
    summarizer.output = Some(128_000);
    let prepared = summarizer.prepare(&history()).unwrap();
    assert_eq!(
        prepared.request.config.as_ref().unwrap().max_output_tokens,
        Some(128_000)
    );
    assert!(
        prepared.request.contents.last().unwrap().parts[0]
            .text()
            .unwrap()
            .contains("Summary output budget: 128000 tokens")
    );
    assert_eq!(prepared.request.contents.last().unwrap().role, "user");
}

#[tokio::test]
async fn skips_tiny_ranges() {
    let (summarizer, model) = summarizer(vec![complete("Unused handoff")], false);
    let mut events = history();
    events[0].set_content(Content::new("user").with_text("Hello"));
    events[1].set_content(Content::new("model").with_text("How can I help?"));
    let original = serialized(&events);
    assert!(
        summarizer
            .summarize_with(&events, SummaryMode::Automatic)
            .await
            .unwrap()
            .is_none()
    );
    assert!(model.requests.lock().unwrap().is_empty());
    assert_eq!(serialized(&events), original);
}

#[tokio::test]
async fn rejects_expanding_outputs() {
    let events = history();
    let original = serialized(&events);
    let tokens = estimate::tokens(&events) as usize;
    for length in [tokens * 2, tokens * 4 + 1, 32 * 1024 + 1] {
        let (summarizer, model) = summarizer(vec![complete(&"x".repeat(length))], false);
        let error = summarizer
            .summarize_with(&events, SummaryMode::Automatic)
            .await
            .unwrap_err();
        assert_eq!(
            failure::classify(&error),
            (Stage::Summary, Reason::NoReduction)
        );
        assert_eq!(model.requests.lock().unwrap().len(), 1);
        assert_eq!(serialized(&events), original);
    }
}

#[test]
fn preserves_canonical_prefix() {
    let (summarizer, _) = summarizer(vec![], false);
    let events = history();
    let expected = history::prefix(&events);
    let prepared = summarizer.prepare(&events).unwrap();
    assert_eq!(
        serde_json::to_vec(&prepared.request.contents[1..prepared.request.contents.len() - 1])
            .unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    assert!(prepared.request.tools.is_empty());
    assert!(prepared.request.previous_response_id.is_none());
}

#[test]
fn preserves_active_declarations() {
    let (mut summarizer, _) = summarizer(vec![], false);
    let tools = [(
        "previously_called".into(),
        json!({
            "name":"previously_called", "parameters":{"type":"object"}
        }),
    )]
    .into();
    summarizer.tools = Some(tools);
    let events = history();
    let ordinary = summarizer.canonical_request(&events, &[], summarizer.tools.clone().unwrap());
    let summary = summarizer.prepare(&events).unwrap().request;
    assert!(summary.tools.contains_key("previously_called"));
    assert_eq!(summary.tools, ordinary.tools);
    assert_eq!(
        serde_json::to_vec(&summary.contents[..ordinary.contents.len()]).unwrap(),
        serde_json::to_vec(&ordinary.contents).unwrap()
    );
}

#[test]
fn preserves_already_masked_prefix() {
    let (mut summarizer, _) = summarizer(vec![], false);
    let mut events = history();
    let mut event = Event::new("historical-result");
    event.set_content(Content {
        role: "function".into(),
        parts: vec![Part::FunctionResponse {
            id: Some("retained-output".into()),
            function_response: adk_core::FunctionResponseData::new(
                "mcp_fixture",
                json!({"text":"Retained evidence ".repeat(800)}),
            ),
            annotations: None,
        }],
    });
    events.push(event);
    let mut ordinary = LlmRequest::new(
        summarizer.model.name(),
        vec![summarizer.preamble.clone().unwrap()],
    );
    ordinary.contents.extend(history::prefix(&events));
    let revisions = model::history::revisions(&events);
    assert!(model::history::mask(&mut ordinary, 0, 1, &revisions).unwrap() > 0);
    summarizer.masks = Some(model::history::masked(&ordinary));
    let summary = summarizer.prepare(&events).unwrap().request;
    assert_eq!(
        serde_json::to_vec(&ordinary.contents).unwrap(),
        serde_json::to_vec(&summary.contents[..ordinary.contents.len()]).unwrap()
    );
}

#[test]
fn prefers_fitting_boundaries() {
    let (summarizer, _) = summarizer(vec![], false);
    let mut events = history();
    events[1].set_content(Content::new("model").with_text("Evidence ".repeat(12_000)));
    assert_eq!(
        summarizer
            .prefix(&events, [2, 1].into_iter(), summarizer.recent())
            .unwrap(),
        Some(1)
    );
}

#[test]
fn includes_exact_capacity_boundary() {
    let (mut summarizer, _) = summarizer(vec![], false);
    let mut summary = Event::new("prior-summary");
    summary.actions.compaction = Some(EventCompaction {
        start_timestamp: summary.timestamp,
        end_timestamp: summary.timestamp,
        compacted_content: Content::new("user").with_text("Earlier handoff"),
    });
    let mut events = vec![summary, Event::new("projection-metadata")];
    events.extend(history());
    let output = summarizer.summary_output();
    let required =
        estimate::request(&summarizer.prepare(&events).unwrap().request).unwrap() + output;
    summarizer.context = Some(required);
    assert_eq!(summarizer.summary_output(), output);
    assert_eq!(
        summarizer
            .prefix(&events, [0, events.len() + 1, 4, 2].into_iter(), 0)
            .unwrap(),
        Some(4)
    );
    summarizer.context = Some(required - 1);
    assert_eq!(
        summarizer.prefix(&events, [4, 2].into_iter(), 0).unwrap(),
        Some(2)
    );
    assert_eq!(
        summarizer
            .prefix(&events, [0, events.len() + 1].into_iter(), 0)
            .unwrap(),
        None
    );
}

#[test]
fn preserves_exact_recent_window() {
    let (summarizer, _) = summarizer(vec![], false);
    let mut events = history();
    let mut tail = Event::new("recent");
    tail.set_content(Content::new("model").with_text("Recent evidence ".repeat(40)));
    events.push(tail);
    let recent = estimate::tokens(&events[2..]);
    assert_eq!(
        summarizer
            .prefix(&events, [3, 2, 1].into_iter(), recent)
            .unwrap(),
        Some(2)
    );
    assert_eq!(
        summarizer
            .prefix(&events, [3, 2, 1].into_iter(), recent + 1)
            .unwrap(),
        Some(1)
    );
}

#[tokio::test]
async fn compares_model_visible_source() {
    let (mut summarizer, model) =
        summarizer(vec![complete(&"handoff evidence ".repeat(1200))], false);
    summarizer.context = Some(128_000);
    let mut events = history();
    events[1]
        .llm_response
        .content
        .as_mut()
        .unwrap()
        .parts
        .push(Part::FunctionCall {
            id: Some("logs".into()),
            name: "mcp_fixture".into(),
            args: json!({}),
            thought_signature: None,
        });
    let mut result = Event::new("completed");
    result.author = "user".into();
    result.set_content(Content {
        role: "function".into(),
        parts: vec![Part::FunctionResponse {
            id: Some("logs".into()),
            function_response: adk_core::FunctionResponseData::new(
                "mcp_fixture",
                json!({
                    "text":"canonical evidence ".repeat(5000)
                }),
            ),
            annotations: None,
        }],
    });
    events.push(result);
    let request = summarizer.prepare(&events).unwrap().request;
    assert!(summarizer.source_tokens(&events, &request).unwrap() < estimate::tokens(&events));
    let error = summarizer
        .summarize_with(&events, SummaryMode::Automatic)
        .await
        .unwrap_err();
    assert_eq!(
        failure::classify(&error),
        (Stage::Summary, Reason::NoReduction)
    );
    assert_eq!(model.requests.lock().unwrap().len(), 1);
}
