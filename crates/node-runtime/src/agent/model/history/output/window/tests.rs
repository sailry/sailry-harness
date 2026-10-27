use super::*;
use adk_core::FunctionResponseData;

fn result(id: &str, name: &str, value: Value) -> Content {
    Content {
        role: "function".into(),
        parts: vec![Part::FunctionResponse {
            id: Some(id.into()),
            function_response: FunctionResponseData::new(name, value),
            annotations: None,
        }],
    }
}

fn value(request: &LlmRequest, index: usize) -> &Value {
    let Part::FunctionResponse {
        function_response, ..
    } = &request.contents[index].parts[0]
    else {
        panic!("expected a canonical tool result");
    };
    &function_response.response
}

#[test]
fn protects_recent_results_and_originals() {
    let original = json!({"text":format!("first {} last", "evidence ".repeat(400))});
    let canonical = LlmRequest::new(
        "fixture",
        vec![
            result("old", "mcp_fixture", original.clone()),
            result(
                "new",
                "delegate",
                json!({"text":"recent evidence".repeat(30)}),
            ),
        ],
    );
    let mut request = canonical.clone();
    assert!(mask(&mut request, 30, 100, &Revisions::new()).unwrap() >= 100);
    assert_eq!(value(&request, 0)["masked"], true);
    assert_eq!(value(&request, 0)["output"]["call"], "old");
    assert_eq!(
        value(&request, 0)["output"]["revision"],
        blake3::hash(original.to_string().as_bytes())
            .to_hex()
            .to_string()
    );
    assert!(
        value(&request, 0)["text"]
            .as_str()
            .unwrap()
            .starts_with("{\"text\":\"first")
    );
    assert!(
        value(&request, 0)["text"]
            .as_str()
            .unwrap()
            .contains(" last")
    );
    assert_eq!(value(&request, 1), value(&canonical, 1));
    assert_eq!(value(&canonical, 0), &original);
}

#[test]
fn skips_small_savings_and_protected_context() {
    let mut request = LlmRequest::new(
        "fixture",
        vec![
            result(
                "skill",
                "load_skill",
                json!({"content":"instruction ".repeat(300)}),
            ),
            result(
                "plan",
                "declared_progress",
                json!({"progress":{
                    "title":"Plan", "steps":[{"description":"Preserve exact plan", "state":"pending"}]
                }}),
            ),
            result("ordinary", "mcp_fixture", json!({"text":"small output"})),
        ],
    );
    let original = serde_json::to_vec(&request).unwrap();
    assert_eq!(mask(&mut request, 0, 100, &Revisions::new()).unwrap(), 0);
    assert_eq!(serde_json::to_vec(&request).unwrap(), original);
}

#[test]
fn masks_commands_with_stream_reader() {
    let original = json!({"sailry_command":{"id":"retained-command"},"stdout":{"text":"log ".repeat(4_000)},"stderr":{"text":"error ".repeat(4_000)}});
    let mut request = LlmRequest::new(
        "fixture",
        vec![result("call", "declared_command", original.clone())],
    );
    assert!(mask(&mut request, 0, 100, &Revisions::new()).unwrap() > 0);
    let masked = value(&request, 0);
    let text = masked["text"].as_str().unwrap();
    assert!(text.contains("read_command id=retained-command"));
    assert!(text.contains("read_tool_output with output"));
    assert_eq!(masked["sailry_command"]["id"], "retained-command");
    assert_eq!(masked["output"]["revision"], hash(&original));
}

#[test]
fn leaves_business_ids_out_of_command_hints() {
    let original = json!({"info":{"id":"business-record"},"text":"business data ".repeat(4_000)});
    let mut request = LlmRequest::new("fixture", vec![result("call", "mcp_fixture", original)]);
    assert!(mask(&mut request, 0, 100, &Revisions::new()).unwrap() > 0);
    let projected = value(&request, 0);
    assert!(
        projected["text"]
            .as_str()
            .unwrap()
            .contains("read_tool_output")
    );
    assert!(!projected["text"].as_str().unwrap().contains("read_command"));
    assert!(projected.get("sailry_command").is_none());
}

#[test]
fn caps_all_results_to_remaining_window() {
    for name in ["mcp_fixture", "declared_delegate", "ordinary_plugin"] {
        let canonical = LlmRequest::new(
            "fixture",
            vec![
                Content::new("user").with_text("Preserve this exact input. ".repeat(100)),
                result("call", name, json!({"text":"result evidence ".repeat(500)})),
            ],
        );
        let mut request = canonical.clone();
        let window = Window {
            context: 2_000,
            output: 256,
        };
        limit(&mut request, Some(window), &Revisions::new()).unwrap();
        assert!(estimate::request(&request).unwrap() <= window.context - window.output);
        assert_eq!(request.contents[0].role, canonical.contents[0].role);
        assert_eq!(request.contents[0].parts, canonical.contents[0].parts);
        assert_eq!(value(&request, 1)["output"]["call"], "call");
        assert!(value(&request, 1).to_string().len() < value(&canonical, 1).to_string().len());
    }
}

#[test]
fn rejects_immutable_context_overflow() {
    let mut request = LlmRequest::new(
        "fixture",
        vec![Content::new("user").with_text("Exact user input. ".repeat(100))],
    );
    let original = serde_json::to_vec(&request).unwrap();
    let error = limit(
        &mut request,
        Some(Window {
            context: 128,
            output: 64,
        }),
        &Revisions::new(),
    )
    .unwrap_err();
    assert_eq!(error.code, "model.sailry.context_overflow");
    assert_eq!(serde_json::to_vec(&request).unwrap(), original);
}

#[test]
fn references_stored_revision() {
    let stored = json!({"text":"stored evidence ".repeat(400), "sailry_context":"Guide"});
    let mut event = adk_core::Event::new("invocation");
    event.set_content(result("stored", "mcp_fixture", stored.clone()));
    let revisions = revisions(std::slice::from_ref(&event));
    let mut visible = stored.clone();
    visible.as_object_mut().unwrap().remove("sailry_context");
    let mut request = LlmRequest::new("fixture", vec![result("stored", "mcp_fixture", visible)]);
    assert!(mask(&mut request, 0, 1, &revisions).unwrap() > 0);
    assert_eq!(
        value(&request, 0)["output"]["revision"],
        blake3::hash(stored.to_string().as_bytes())
            .to_hex()
            .to_string()
    );
}

#[test]
fn repeats_masks_without_nesting() {
    let original = json!({"text":"older evidence ".repeat(400)});
    let mut request = LlmRequest::new(
        "fixture",
        vec![result("old", "mcp_fixture", original.clone())],
    );
    mask(&mut request, 0, 1, &Revisions::new()).unwrap();
    let calls = masked(&request);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls.first().unwrap().0, "old");
    let projected = value(&request, 0).clone();
    remask(&mut request, &calls, &Revisions::new());
    assert_eq!(value(&request, 0), &projected);
    let mut fresh = LlmRequest::new("fixture", vec![result("old", "mcp_fixture", original)]);
    remask(&mut fresh, &calls, &Revisions::new());
    assert_eq!(value(&fresh, 0), &projected);
}

#[test]
fn scopes_masks_to_result_revisions() {
    let old = json!({"text":"Old retained evidence ".repeat(400)});
    let new = json!({"text":"New protected evidence ".repeat(400)});
    let contents = vec![
        result("reused", "mcp_fixture", old.clone()),
        result("reused", "mcp_fixture", new),
    ];
    let events = contents
        .iter()
        .cloned()
        .map(|content| {
            let mut event = adk_core::Event::new("captured");
            event.set_content(content);
            event
        })
        .collect::<Vec<_>>();
    let revisions = revisions(&events);
    let mut request = LlmRequest::new("fixture", contents.clone());
    assert!(mask(&mut request, 1, 1, &revisions).unwrap() > 0);
    let expected = hash(&old);
    assert_eq!(value(&request, 0)["output"]["revision"], expected);
    assert_eq!(
        source(value(&request, 0), "reused", &revisions)["revision"],
        expected
    );
    assert!(value(&request, 1).get("masked").is_none());
    let masks = masked(&request);
    let saved = serde_json::to_vec(&request.contents).unwrap();
    let mut restored = LlmRequest::new("fixture", contents);
    remask(&mut restored, &masks, &revisions);
    assert_eq!(serde_json::to_vec(&restored.contents).unwrap(), saved);
}

#[test]
fn keeps_results_that_fit_the_window() {
    let large = json!({"text":"large evidence ".repeat(4000)});
    let mut request = LlmRequest::new(
        "fixture",
        vec![
            result("large", "ordinary_fixture", large.clone()),
            result("small", "ordinary_fixture", json!({"text":"small"})),
        ],
    );
    let roomy = Window {
        context: 100_000,
        output: 1_000,
    };
    assert_eq!(
        limit(&mut request, Some(roomy), &Revisions::new()).unwrap(),
        None
    );
    assert_eq!(value(&request, 0), &large);
    let tight = Window {
        context: 4_000,
        output: 1_000,
    };
    let fingerprint = limit(&mut request, Some(tight), &Revisions::new()).unwrap();
    assert!(fingerprint.is_some());
    assert!(estimate::request(&request).unwrap() <= 3_000);
    assert_eq!(value(&request, 1), &json!({"text":"small"}));
    let mut repeated = LlmRequest::new(
        "fixture",
        vec![
            result("large", "ordinary_fixture", large),
            result("small", "ordinary_fixture", json!({"text":"small"})),
        ],
    );
    assert_eq!(
        limit(&mut repeated, Some(tight), &Revisions::new()).unwrap(),
        fingerprint
    );
}

#[test]
fn deduplicates_before_context_measurement() {
    let output = json!({"text":"canonical evidence ".repeat(2000)});
    let canonical = json!({"output":output,"text":[output.to_string()]});
    let mut request = LlmRequest::new(
        "fixture",
        vec![result("call", "mcp_fixture", canonical.clone())],
    );
    let original = estimate::request(&request).unwrap();
    let mut prepared = prepare(&mut request, &Revisions::new());
    assert!(prepared.tokens(&request).unwrap() < original * 3 / 4);
    assert_eq!(
        prepared.tokens(&request).unwrap(),
        estimate::request(&request).unwrap()
    );
    assert!(value(&request, 0).get("text").is_none());
    assert!(prepared.mask(&mut request, 0, 1).unwrap() > 0);
    assert_eq!(value(&request, 0)["output"]["revision"], hash(&canonical));
    assert_eq!(
        prepared.tokens(&request).unwrap(),
        estimate::request(&request).unwrap()
    );
}

#[test]
fn preserves_revisions_after_prepared_invalidation() {
    let output = json!({"text":"canonical evidence ".repeat(200)});
    let canonical = json!({"output":output,"text":[output.to_string()]});
    let mut request = LlmRequest::new(
        "fixture",
        vec![result("call", "mcp_fixture", canonical.clone())],
    );
    prepare(&mut request, &Revisions::new())
        .limit(&mut request, None)
        .unwrap();
    let tight = Window {
        context: 1000,
        output: 100,
    };
    assert!(!take_prepared(&mut request, Some(tight)).unwrap());
    prepare(&mut request, &Revisions::new())
        .limit(&mut request, Some(tight))
        .unwrap();
    assert_eq!(value(&request, 0)["output"]["revision"], hash(&canonical));
}

#[test]
fn measures_pending_revisions_in_canonical_order() {
    let older = json!({"text":"older evidence ".repeat(500)});
    let latest = json!({"text":"latest evidence"});
    let mut request = LlmRequest::new(
        "fixture",
        vec![
            result("reused", "ordinary", older),
            result("reused", "ordinary", latest.clone()),
        ],
    );
    let mut outputs = prepare(&mut request, &Revisions::new());
    outputs.mask(&mut request, 1, 1).unwrap();
    let mut event = adk_core::Event::new("pending");
    event.set_content(result("reused", "ordinary", latest));
    assert_eq!(
        outputs.pending_tokens(std::slice::from_ref(&event)),
        estimate::tokens(&[event])
    );
}

#[test]
fn verifies_prepared_request_boundaries() {
    let window = Window {
        context: 10_000,
        output: 1000,
    };
    let mut request = LlmRequest::new(
        "fixture",
        vec![result("call", "ordinary", json!({"text":"evidence"}))],
    );
    prepare(&mut request, &Revisions::new())
        .limit(&mut request, Some(window))
        .unwrap();
    let mut unchanged = request.clone();
    assert!(take_prepared(&mut unchanged, Some(window)).unwrap());
    assert!(
        unchanged
            .config
            .as_ref()
            .unwrap()
            .extensions
            .get(PREPARED)
            .is_none()
    );
    let mut changed = request.clone();
    if let Part::FunctionResponse {
        function_response, ..
    } = &mut changed.contents[0].parts[0]
    {
        function_response.response["text"] = json!("different content");
    }
    assert!(!take_prepared(&mut changed, Some(window)).unwrap());
    assert!(
        !take_prepared(
            &mut request,
            Some(Window {
                context: 100,
                output: 50
            })
        )
        .unwrap()
    );
}
