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
    assert!(
        prepare(&mut request, None)
            .mask(&mut request, 30, 100)
            .unwrap()
            >= 100
    );
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
    assert_eq!(
        prepare(&mut request, None)
            .mask(&mut request, 0, 100)
            .unwrap(),
        0
    );
    assert_eq!(serde_json::to_vec(&request).unwrap(), original);
}

#[test]
fn masks_commands_with_stream_reader() {
    let original = json!({"sailry_command":{"id":"retained-command"},"stdout":{"text":"log ".repeat(4_000)},"stderr":{"text":"error ".repeat(4_000)}});
    let mut request = LlmRequest::new(
        "fixture",
        vec![result("call", "declared_command", original.clone())],
    );
    assert!(
        prepare(&mut request, None)
            .mask(&mut request, 0, 100)
            .unwrap()
            > 0
    );
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
    assert!(
        prepare(&mut request, None)
            .mask(&mut request, 0, 100)
            .unwrap()
            > 0
    );
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
        prepare(&mut request, None)
            .limit(&mut request, Some(window))
            .unwrap();
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
    let error = prepare(&mut request, None)
        .limit(
            &mut request,
            Some(Window {
                context: 128,
                output: 64,
            }),
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
    assert!(
        prepare(&mut request, Some(&revisions))
            .mask(&mut request, 0, 1)
            .unwrap()
            > 0
    );
    assert_eq!(
        value(&request, 0)["output"]["revision"],
        blake3::hash(stored.to_string().as_bytes())
            .to_hex()
            .to_string()
    );
}

#[test]
fn hashes_only_referenced_canonical_results() {
    let mut events = Vec::new();
    for id in ["first", "second", "unused"] {
        let mut event = adk_core::Event::new("stored");
        event.set_content(result(
            id,
            "ordinary",
            json!({"text":id.repeat(1000),"sailry_context":"Owner"}),
        ));
        events.push(event);
    }
    let revisions = revisions(&events);
    assert!(
        revisions
            .values
            .values()
            .flatten()
            .all(|(_, cached)| cached.get().is_none())
    );
    let mut request = LlmRequest::new(
        "fixture",
        vec![result(
            "second",
            "ordinary",
            json!({"text":"projected evidence ".repeat(1000)}),
        )],
    );
    let mut outputs = prepare(&mut request, Some(&revisions));
    assert!(outputs.mask(&mut request, 0, 1).unwrap() > 0);
    let Part::FunctionResponse {
        function_response, ..
    } = &events[1].content().unwrap().parts[0]
    else {
        unreachable!()
    };
    assert_eq!(
        value(&request, 0)["output"]["revision"],
        hash(&function_response.response)
    );
    for (id, values) in &revisions.values {
        assert_eq!(values[0].1.get().is_some(), *id == "second");
    }
    let encoded = serde_json::to_vec(&request.contents).unwrap();
    let masks = masked(&request);
    outputs.remask(&mut request, &masks);
    assert_eq!(serde_json::to_vec(&request.contents).unwrap(), encoded);
}

#[test]
fn repeats_masks_without_nesting() {
    let original = json!({"text":"older evidence ".repeat(400)});
    let mut request = LlmRequest::new(
        "fixture",
        vec![result("old", "mcp_fixture", original.clone())],
    );
    prepare(&mut request, None)
        .mask(&mut request, 0, 1)
        .unwrap();
    let calls = masked(&request);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls.first().unwrap().0, "old");
    let projected = value(&request, 0).clone();
    prepare(&mut request, None).remask(&mut request, &calls);
    assert_eq!(value(&request, 0), &projected);
    let mut fresh = LlmRequest::new("fixture", vec![result("old", "mcp_fixture", original)]);
    prepare(&mut fresh, None).remask(&mut fresh, &calls);
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
    assert!(
        prepare(&mut request, Some(&revisions))
            .mask(&mut request, 1, 1)
            .unwrap()
            > 0
    );
    let expected = hash(&old);
    assert_eq!(value(&request, 0)["output"]["revision"], expected);
    assert_eq!(
        encoded_source(
            value(&request, 0),
            "reused",
            Some(&revisions),
            &value(&request, 0).to_string()
        )["revision"],
        expected
    );
    assert!(value(&request, 1).get("masked").is_none());
    let masks = masked(&request);
    let saved = serde_json::to_vec(&request.contents).unwrap();
    let mut restored = LlmRequest::new("fixture", contents);
    prepare(&mut restored, Some(&revisions)).remask(&mut restored, &masks);
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
        prepare(&mut request, None)
            .limit(&mut request, Some(roomy))
            .unwrap(),
        None
    );
    assert_eq!(value(&request, 0), &large);
    let tight = Window {
        context: 4_000,
        output: 1_000,
    };
    let fingerprint = prepare(&mut request, None)
        .limit(&mut request, Some(tight))
        .unwrap();
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
        prepare(&mut repeated, None)
            .limit(&mut repeated, Some(tight))
            .unwrap(),
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
    let mut prepared = prepare(&mut request, None);
    assert!(prepared.tokens(&request).unwrap() < original * 3 / 4);
    let mut wire = request.clone();
    let Part::FunctionResponse {
        function_response, ..
    } = &mut wire.contents[0].parts[0]
    else {
        unreachable!()
    };
    assert_eq!(
        function_response
            .response
            .as_object_mut()
            .unwrap()
            .remove("sailry_output"),
        Some(json!({"call":"call","revision":hash(&canonical)}))
    );
    assert_eq!(
        prepared.tokens(&request).unwrap(),
        estimate::request(&wire).unwrap()
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
    prepare(&mut request, None)
        .limit(&mut request, None)
        .unwrap();
    let tight = Window {
        context: 1000,
        output: 100,
    };
    assert!(!take_prepared(&mut request, Some(tight)));
    prepare(&mut request, None)
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
    let mut outputs = prepare(&mut request, None);
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
    prepare(&mut request, None)
        .limit(&mut request, Some(window))
        .unwrap();
    let mut unchanged = request.clone();
    assert!(take_prepared(&mut unchanged, Some(window)));
    assert!(
        unchanged
            .config
            .as_ref()
            .unwrap()
            .extensions
            .get(PREPARED)
            .is_none()
    );
    let mut rebuilt = request.clone();
    clear_prepared(&mut rebuilt);
    assert!(!take_prepared(&mut rebuilt, Some(window)));
    let mut changed = request.clone();
    changed.config.as_mut().unwrap().max_output_tokens = Some(200);
    assert!(!take_prepared(&mut changed, Some(window)));
    assert!(!take_prepared(
        &mut request,
        Some(Window {
            context: 100,
            output: 50
        })
    ));
}

#[test]
fn resolves_anonymous_parallel_results() {
    let originals = ["first", "second"].map(|label| {
        let output = json!({"label":label,"text":"Canonical evidence 中文 🙂 ".repeat(4000)});
        json!({"output":output,"text":[output.to_string()]})
    });
    let call = Content {
        role: "model".into(),
        parts: ["first", "second"]
            .into_iter()
            .map(|label| Part::ProviderData {
                provider_data: adk_core::ProviderData {
                    origin: None,
                    parts: Box::new([Part::FunctionCall {
                        id: Some(format!("adk-call-{label}")),
                        name: "mcp_fixture".into(),
                        args: json!({"path":label}),
                        thought_signature: Some("signature".into()),
                    }]),
                },
            })
            .collect(),
    };
    let first = result("adk-call-first", "mcp_fixture", originals[0].clone());
    let second = result("adk-call-second", "mcp_fixture", originals[1].clone());
    let events = [call.clone(), second.clone(), first.clone()]
        .into_iter()
        .map(|content| {
            let mut event = adk_core::Event::new("captured");
            event.set_content(content);
            event
        })
        .collect::<Vec<_>>();
    let stored = serde_json::to_vec(&events).unwrap();
    let revisions = revisions(&events);
    let mut request = LlmRequest::new("fixture", vec![call, first, second]);
    request
        .contents
        .iter_mut()
        .for_each(Content::strip_generated_call_ids);
    let live = request.clone();
    let mut outputs = prepare(&mut request, Some(&revisions));
    assert_eq!(outputs.slots.len(), 2);
    for (index, label) in ["first", "second"].into_iter().enumerate() {
        assert_eq!(outputs.slots[index].arguments, Some(json!({"path":label})));
        assert!(value(&request, index + 1).get("text").is_none());
        assert_eq!(
            value(&request, index + 1)["sailry_output"]["revision"],
            hash(&originals[index])
        );
    }
    assert!(outputs.tokens(&request).unwrap() < estimate::request(&live).unwrap());
    assert!(outputs.mask(&mut request, 0, 1).unwrap() > 0);
    let masks = masked(&request);
    assert_eq!(masks.len(), 2);
    for (index, label) in ["first", "second"].into_iter().enumerate() {
        assert_eq!(
            value(&request, index + 1)["output"]["call"],
            format!("adk-call-{label}")
        );
        assert_eq!(
            value(&request, index + 1)["output"]["revision"],
            hash(&originals[index])
        );
    }
    let mut replay = live;
    prepare(&mut replay, Some(&revisions)).remask(&mut replay, &masks);
    assert_eq!(
        serde_json::to_vec(&request.contents).unwrap(),
        serde_json::to_vec(&replay.contents).unwrap()
    );
    outputs
        .limit(
            &mut request,
            Some(Window {
                context: 10_000,
                output: 100,
            }),
        )
        .unwrap();
    assert!(
        request
            .contents
            .iter()
            .flat_map(|content| &content.parts)
            .all(|part| match part.provider_part() {
                Part::FunctionCall { id, .. } | Part::FunctionResponse { id, .. } => id.is_none(),
                _ => true,
            })
    );
    assert_eq!(serde_json::to_vec(&events).unwrap(), stored);
}

#[test]
fn omits_ambiguous_search_arguments() {
    let original = json!({"kind":"search_results","data":sailry_protocol::SearchResults {
        matches: vec![sailry_protocol::SearchMatch {
            path:"fixture.rs".into(), line_number:1,
            line:format!("{}first needle {}second needle", "before ".repeat(200), "middle ".repeat(200)),
            matched:true, truncated:false,
        }], ..Default::default()
    }});
    let calls = Content {
        role: "model".into(),
        parts: ["first", "second"]
            .into_iter()
            .map(|label| Part::FunctionCall {
                id: Some(format!("adk-call-{label}")),
                name: "search".into(),
                args: json!({"query":format!("{label} needle")}),
                thought_signature: None,
            })
            .collect(),
    };
    let contents = vec![
        calls,
        result("adk-call-first", "search", original.clone()),
        result("adk-call-second", "search", original.clone()),
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
    let mut request = LlmRequest::new("fixture", contents);
    request
        .contents
        .iter_mut()
        .for_each(Content::strip_generated_call_ids);
    let mut outputs = prepare(&mut request, Some(&revisions));
    assert_eq!(outputs.slots.len(), 2);
    assert!(outputs.slots.iter().all(|slot| slot.arguments.is_none()));
    outputs.limit(&mut request, None).unwrap();
    for index in 1..=2 {
        let projected = value(&request, index);
        assert_eq!(projected["output"]["revision"], hash(&original));
        assert!(projected["text"].as_str().unwrap().contains("before"));
        assert!(!projected["text"].as_str().unwrap().contains("needle"));
    }
}

#[test]
fn preserves_equal_result_mask_ownership() {
    let original = json!({"text":"Repeated canonical evidence 中文 🙂 ".repeat(400)});
    let canonical = ["old", "recent", "latest"]
        .into_iter()
        .flat_map(|label| {
            [
                Content {
                    role: "model".into(),
                    parts: vec![Part::FunctionCall {
                        id: Some(format!("adk-call-{label}")),
                        name: "ordinary".into(),
                        args: json!({"query":label}),
                        thought_signature: None,
                    }],
                },
                result(&format!("adk-call-{label}"), "ordinary", original.clone()),
            ]
        })
        .collect::<Vec<_>>();
    let events = canonical
        .iter()
        .cloned()
        .map(|content| {
            let mut event = adk_core::Event::new("captured");
            event.set_content(content);
            event
        })
        .collect::<Vec<_>>();
    let initial = revisions(&events[..4]);
    let mut request = LlmRequest::new("fixture", canonical[..4].to_vec());
    request
        .contents
        .iter_mut()
        .for_each(Content::strip_generated_call_ids);
    let mut outputs = prepare(&mut request, Some(&initial));
    assert!(outputs.mask(&mut request, 1, 1).unwrap() > 0);
    let masks = masked(&request);
    assert_eq!(
        masks,
        BTreeSet::from([("adk-call-old".into(), hash(&original))])
    );
    assert_eq!(value(&request, 1)["masked"], true);
    assert!(value(&request, 3).get("masked").is_none());

    let current = revisions(&events);
    for retained in [false, true] {
        let mut next = LlmRequest::new("fixture", canonical.clone());
        next.contents
            .iter_mut()
            .for_each(Content::strip_generated_call_ids);
        if retained {
            next.contents[1] = request.contents[1].clone();
        }
        prepare(&mut next, Some(&current)).remask(&mut next, &masks);
        assert_eq!(value(&next, 1)["masked"], true);
        assert_eq!(value(&next, 1)["output"]["call"], "adk-call-old");
        for (index, label) in [(3, "recent"), (5, "latest")] {
            assert!(value(&next, index).get("masked").is_none());
            assert_eq!(value(&next, index)["text"], original["text"]);
            assert_eq!(
                value(&next, index)["sailry_output"]["call"],
                format!("adk-call-{label}")
            );
        }
        let encoded = serde_json::to_vec(&next.contents).unwrap();
        prepare(&mut next, Some(&current)).remask(&mut next, &masks);
        assert_eq!(serde_json::to_vec(&next.contents).unwrap(), encoded);
    }
}
