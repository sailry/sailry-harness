#![cfg(feature = "openai")]

use adk_core::{Content, Llm, LlmRequest, LlmResponse, Part};
use adk_model::openai::{OpenAIResponsesClient, OpenAIResponsesConfig};
use futures::TryStreamExt;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn completion(output: Value) -> Value {
    json!({
        "type": "response.completed", "sequence_number": 10,
        "response": {
            "id": "resp_test", "object": "response", "created_at": 0,
            "model": "test-model", "status": "completed", "output": output,
            "usage": {
                "input_tokens": 100, "input_tokens_details": {"cached_tokens": 42},
                "output_tokens": 20, "output_tokens_details": {"reasoning_tokens": 5},
                "total_tokens": 120
            }
        }
    })
}

fn message(id: &str, content: Value) -> Value {
    json!({
        "type":"message", "id":id, "role":"assistant", "status":"completed", "content":content
    })
}

fn text(text: &str) -> Value {
    json!({"type":"output_text", "text":text, "annotations":[]})
}

fn delta(id: &str, output_index: u32, content_index: u32, text: &str) -> Value {
    json!({
        "type":"response.output_text.delta", "sequence_number":1, "item_id":id,
        "output_index":output_index, "content_index":content_index, "delta":text
    })
}

fn done(id: &str, output_index: u32, content_index: u32, text: &str) -> Value {
    json!({
        "type":"response.output_text.done", "sequence_number":2, "item_id":id,
        "output_index":output_index, "content_index":content_index, "text":text, "logprobs":[]
    })
}

fn text_parts(results: &[LlmResponse]) -> Vec<&str> {
    results
        .iter()
        .filter_map(|response| response.content.as_ref())
        .flat_map(|content| &content.parts)
        .filter_map(|part| if let Part::Text { text } = part { Some(text.as_str()) } else { None })
        .collect()
}

async fn responses(events: &[Value], open_responses_mode: bool, prefix: &str) -> Vec<LlmResponse> {
    let results = try_responses(events, open_responses_mode, prefix)
        .await
        .expect("SSE events must deserialize");
    assert_eq!(results.iter().filter(|response| response.turn_complete).count(), 1);
    results
        .into_iter()
        // Lifecycle activity is independent of these content/terminal assertions.
        .filter(|response| !empty_activity(response))
        .collect()
}

fn empty_activity(response: &LlmResponse) -> bool {
    response.partial
        && !response.turn_complete
        && !response.interrupted
        && response.content.is_none()
        && response.error_code.is_none()
        && response.error_message.is_none()
        && response.usage_metadata.is_none()
        && response.finish_reason.is_none()
        && response.citation_metadata.is_none()
        && response.provider_metadata.is_none()
        && response.interaction_id.is_none()
}

mod http_errors {
    use super::*;

    #[tokio::test]
    async fn preserves_detail_and_status() {
        for status in [400, 401, 429, 503] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/responses"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .set_body_json(json!({"detail":"Selected model is not supported"})),
                )
                .expect(1)
                .mount(&server)
                .await;
            let client = OpenAIResponsesClient::new(
                OpenAIResponsesConfig::new("test-key", "test-model")
                    .with_base_url(server.uri())
                    .with_open_responses_mode(true),
            )
            .unwrap()
            .with_retry_config(adk_model::retry::RetryConfig::disabled());
            let request =
                LlmRequest::new("test-model", vec![Content::new("user").with_text("test")]);
            let result = match client.generate_content(request, true).await {
                Ok(stream) => stream.try_collect::<Vec<_>>().await,
                Err(error) => Err(error),
            };
            let error = result.unwrap_err();
            assert_eq!(error.details.upstream_status_code, Some(status));
            assert!(error.to_string().contains("Selected model is not supported"));
            assert_ne!(error.code, "model.openai_responses.parse");
        }
    }
}

mod omitted_output {
    use super::*;

    #[tokio::test]
    async fn recovers_completed_calls_once() {
        for compatible in [false, true] {
            let item = json!({"type":"function_call", "id":"fc_a", "call_id":"call_a",
                "name":"read_file", "arguments":r#"{"path":"first.txt"}"#, "status":"completed"});
            let results = responses(
                &[
                    json!({"type":"response.output_item.done", "sequence_number":1,
                    "output_index":0, "item":item}),
                    json!({"type":"response.output_item.done", "sequence_number":2,
                    "output_index":0, "item":item}),
                    completion(json!([])),
                ],
                compatible,
                "",
            )
            .await;
            let terminal = results.last().unwrap();
            assert_eq!(terminal.provider_metadata.as_ref().unwrap()["content_complete"], true);
            assert_eq!(
                terminal.content.as_ref().unwrap().parts,
                [Part::FunctionCall {
                    id: Some("call_a".into()),
                    name: "read_file".into(),
                    args: json!({"path":"first.txt"}),
                    thought_signature: None,
                }]
            );
        }
    }

    #[tokio::test]
    async fn preserves_explicit_terminal_output() {
        for compatible in [false, true] {
            let item = json!({"type":"function_call", "id":"fc_a", "call_id":"call_a",
                "name":"read_file", "arguments":"{}", "status":"completed"});
            let results = responses(
                &[
                    json!({"type":"response.output_item.done", "sequence_number":1,
                    "output_index":0, "item":item}),
                    completion(json!([message("answer", json!([text("No tool needed")]))])),
                ],
                compatible,
                "",
            )
            .await;
            assert_eq!(text_parts(&results), ["No tool needed"]);
            assert!(
                !results
                    .last()
                    .unwrap()
                    .content
                    .as_ref()
                    .unwrap()
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::FunctionCall { .. }))
            );
        }
    }
}

async fn try_responses(
    events: &[Value],
    open_responses_mode: bool,
    prefix: &str,
) -> Result<Vec<LlmResponse>, adk_core::AdkError> {
    let server = MockServer::start().await;
    let mut body = prefix.to_string();
    for event in events {
        body.push_str(&format!("data: {event}\n\n"));
    }
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .expect(1)
        .mount(&server)
        .await;
    let client = OpenAIResponsesClient::new(
        OpenAIResponsesConfig::new("test-key", "test-model")
            .with_base_url(server.uri())
            .with_open_responses_mode(open_responses_mode),
    )
    .unwrap();
    let request = LlmRequest::new("test-model", vec![Content::new("user").with_text("read files")]);
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        client.generate_content(request, true).await.unwrap().try_collect::<Vec<_>>().await
    })
    .await
    .expect("stream must terminate");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&requests[0].body).unwrap()["stream"], true);
    result
}

#[tokio::test]
async fn compatible_stream_rejects_conflicting_tool_identities_before_completion() {
    for conflict in [
        json!({"type":"response.output_item.added", "sequence_number":3, "output_index":0,
            "item":{"type":"function_call","id":"fc_b","call_id":"call_b","name":"read_file","arguments":"","status":"in_progress"}}),
        json!({"type":"response.output_item.done", "sequence_number":3, "output_index":0,
            "item":{"type":"function_call","id":"fc_b","call_id":"call_b","name":"read_file","arguments":"","status":"completed"}}),
        json!({"type":"response.function_call_arguments.delta", "sequence_number":3, "output_index":0,
            "item_id":"fc_b","delta":" "}),
        json!({"type":"response.function_call_arguments.done", "sequence_number":3, "output_index":0,
            "item_id":"fc_b","arguments":""}),
    ] {
        let events = [
            json!({"type":"response.output_item.added", "sequence_number":1, "output_index":0,
                "item":{"type":"function_call","id":"fc_a","call_id":"call_a","name":"read_file","arguments":"","status":"in_progress"}}),
            json!({"type":"response.function_call_arguments.delta", "sequence_number":2, "output_index":0,
                "item_id":"fc_a","delta":r#"{"path":"A"}"#}),
            conflict.clone(),
            json!({"type":"response.output_item.done", "sequence_number":4, "output_index":0,
                "item":{"type":"function_call","id":"fc_b","call_id":"call_b","name":"read_file","arguments":"","status":"completed"}}),
            completion(
                json!([{"type":"function_call","id":"fc_b","call_id":"call_b","name":"read_file"}]),
            ),
        ];
        assert!(
            try_responses(&events, true, "").await.is_err(),
            "conflicting event was accepted: {conflict}"
        );
    }
}

#[tokio::test]
async fn compatible_stream_restores_blank_done_and_completed_arguments() {
    for blank in ["", " \t\r\n"] {
        let results = responses(&[
            json!({"type":"response.function_call_arguments.delta", "sequence_number":1, "output_index":0,
                "item_id":"fc_a","delta":r#"{"path":"first.txt"}"#}),
            json!({"type":"response.function_call_arguments.done", "sequence_number":2, "output_index":0,
                "item_id":"fc_a","arguments":blank}),
            json!({"type":"response.output_item.done", "sequence_number":3, "output_index":0,
                "item":{"type":"function_call","id":"fc_a","call_id":"call_a","name":"read_file","arguments":blank,"status":"completed"}}),
            completion(json!([{"type":"function_call","id":"fc_a","call_id":"call_a","name":"read_file","arguments":blank}])),
        ], true, "").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].error_code, None);
        assert_eq!(
            results[0].content.as_ref().unwrap().parts,
            vec![Part::FunctionCall {
                id: Some("call_a".into()),
                name: "read_file".into(),
                args: json!({"path":"first.txt"}),
                thought_signature: None,
            }]
        );
        assert_eq!(
            results[0].usage_metadata.as_ref().unwrap().cache_read_input_token_count,
            Some(42)
        );
    }
}

#[tokio::test]
async fn standard_mode_does_not_apply_compatibility_identity_rules() {
    let results = responses(&[
        json!({"type":"response.output_item.added", "sequence_number":1, "output_index":0,
            "item":{"type":"function_call","id":"fc_a","call_id":"call_a","name":"read_file","arguments":"","status":"in_progress"}}),
        json!({"type":"response.function_call_arguments.delta", "sequence_number":2, "output_index":0,
            "item_id":"fc_a","delta":r#"{"path":"first.txt"}"#}),
        json!({"type":"response.output_item.done", "sequence_number":3, "output_index":0,
            "item":{"type":"function_call","id":"fc_b","call_id":"call_b","name":"read_file","arguments":r#"{"path":"second.txt"}"#,"status":"completed"}}),
        completion(json!([{"type":"function_call","id":"fc_b","call_id":"call_b","name":"read_file",
            "arguments":r#"{"path":"second.txt"}"#,"status":"completed"}])),
    ], false, "").await;
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].content.as_ref().unwrap().parts,
        vec![Part::FunctionCall {
            id: Some("call_b".into()),
            name: "read_file".into(),
            args: json!({"path":"second.txt"}),
            thought_signature: None,
        }]
    );
}

#[tokio::test]
async fn streamed_invalid_arguments_preserve_errors_and_usage_in_both_modes() {
    for compatible in [false, true] {
        let results = responses(
            &[completion(json!([{
                "type": "function_call", "id": "fc_bad", "call_id": "call_bad",
                "name": "read_file", "arguments": "{invalid", "status": "completed"
            }]))],
            compatible,
            "",
        )
        .await;
        assert_eq!(results.len(), 1);
        let result = &results[0];
        assert_eq!(
            result.error_code.as_deref(),
            Some("model.openai_responses.invalid_tool_arguments")
        );
        assert!(result.error_message.as_deref().is_some_and(|message| !message.is_empty()));
        assert!(result.turn_complete);
        assert!(!result.partial);
        assert_eq!(result.usage_metadata.as_ref().unwrap().cache_read_input_token_count, Some(42));
        assert_eq!(
            result.provider_metadata.as_ref().unwrap()["openai"]["response_id"],
            "resp_test"
        );
        assert!(result.content.as_ref().is_none_or(|content| content.parts.is_empty()));
    }
}

#[tokio::test]
async fn compatible_stream_restores_only_the_matching_tools_arguments() {
    let results = responses(
        &[
            json!({
                "type": "response.function_call_arguments.delta", "sequence_number": 1,
                "output_index": 1, "item_id": "fc_b", "delta": r#"{"path":"second.txt"}"#
            }),
            completion(json!([
                {"type": "function_call", "id": "fc_a", "call_id": "call_a",
                    "name": "read_file", "arguments": r#"{"path":"first.txt"}"#},
                {"type": "function_call", "id": "fc_b", "call_id": "call_b", "name": "read_file"}
            ])),
        ],
        true,
        "data:\n\ndata: \r\n\r\n: keepalive\n\n",
    )
    .await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].error_code, None);
    assert_eq!(
        results[0].content.as_ref().unwrap().parts,
        vec![
            Part::FunctionCall {
                id: Some("call_a".into()),
                name: "read_file".into(),
                args: json!({"path": "first.txt"}),
                thought_signature: None
            },
            Part::FunctionCall {
                id: Some("call_b".into()),
                name: "read_file".into(),
                args: json!({"path": "second.txt"}),
                thought_signature: None
            },
        ]
    );
}

mod text_recovery {
    use super::*;

    #[tokio::test]
    async fn completed_text_without_deltas() {
        for compatible in [false, true] {
            let results = responses(
                &[completion(json!([
                    message("first", json!([text("First")])),
                    message("second", json!([text("Second")]))
                ]))],
                compatible,
                "",
            )
            .await;
            assert_eq!(results.len(), 1);
            assert_eq!(text_parts(&results), ["First", "Second"]);
            assert!(!results[0].partial);
            assert!(results[0].turn_complete);
            assert_eq!(results[0].usage_metadata.as_ref().unwrap().total_token_count, 120);
        }
    }

    #[tokio::test]
    async fn done_text_without_completed_content() {
        for compatible in [false, true] {
            let results =
                responses(&[done("msg", 0, 0, "Recovered"), completion(json!([]))], compatible, "")
                    .await;
            assert_eq!(text_parts(&results), ["Recovered"]);
            assert!(results[0].partial);
            assert!(results.last().unwrap().turn_complete);
        }
    }

    #[tokio::test]
    async fn partial_text_from_each_final_event() {
        let item = message("msg", json!([text("前后")]));
        for compatible in [false, true] {
            for final_event in [
                done("msg", 0, 0, "前后"),
                json!({"type":"response.content_part.done", "sequence_number":2,
                    "item_id":"msg", "output_index":0, "content_index":0, "part":text("前后")}),
                json!({"type":"response.output_item.done", "sequence_number":2,
                    "output_index":0, "item":item}),
                completion(json!([item])),
            ] {
                let mut events = vec![delta("msg", 0, 0, "前")];
                let terminal = final_event["type"] == "response.completed";
                events.push(final_event);
                if !terminal {
                    events.push(completion(json!([item])));
                }
                let results = responses(&events, compatible, "").await;
                assert_eq!(text_parts(&results), ["前", "后"]);
            }
        }
    }

    #[tokio::test]
    async fn repeated_final_events_do_not_duplicate() {
        for compatible in [false, true] {
            let item = message("msg", json!([text("Answer")]));
            let results = responses(
                &[
                    delta("msg", 0, 0, "Answer"),
                    done("msg", 0, 0, "Answer"),
                    done("msg", 0, 0, "Answer"),
                    json!({"type":"response.content_part.done", "sequence_number":3,
                    "item_id":"msg", "output_index":0, "content_index":0, "part":text("Answer")}),
                    json!({"type":"response.output_item.done", "sequence_number":4,
                    "output_index":0, "item":item}),
                    completion(json!([item])),
                ],
                compatible,
                "",
            )
            .await;
            assert_eq!(text_parts(&results), ["Answer"]);
            assert!(results.last().unwrap().content.is_none());
            assert_eq!(
                results
                    .last()
                    .unwrap()
                    .usage_metadata
                    .as_ref()
                    .unwrap()
                    .cache_read_input_token_count,
                Some(42)
            );
        }
    }

    #[tokio::test]
    async fn items_and_parts_are_independent() {
        for compatible in [false, true] {
            let first = message("first", json!([text("First"), text("Second")]));
            let second = message("second", json!([text("Third")]));
            let results = responses(
                &[
                    delta("first", 0, 0, "First"),
                    json!({"type":"response.output_item.done", "sequence_number":2,
                    "output_index":0, "item":first}),
                    done("second", 1, 0, "Third"),
                    completion(json!([first, second])),
                ],
                compatible,
                "",
            )
            .await;
            assert_eq!(text_parts(&results), ["First", "Second", "Third"]);
        }
    }

    #[tokio::test]
    async fn repeated_message_ids_remain_distinct() {
        for compatible in [false, true] {
            let results = responses(
                &[
                    delta("msg", 0, 0, "First"),
                    completion(json!([
                        message("msg", json!([text("First")])),
                        message("msg", json!([text("Second")]))
                    ])),
                ],
                compatible,
                "",
            )
            .await;
            assert_eq!(text_parts(&results), ["First", "Second"]);
        }
    }

    #[tokio::test]
    async fn corrected_text_replaces_prior_deltas() {
        for compatible in [false, true] {
            let results = responses(
                &[
                    delta("msg", 0, 0, "Draft"),
                    done("msg", 0, 0, "Final"),
                    completion(json!([message("msg", json!([text("Final")]))])),
                ],
                compatible,
                "",
            )
            .await;
            assert_eq!(text_parts(&results), ["Draft", "Final"]);
            let final_response = results.last().unwrap();
            assert_eq!(
                final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                true
            );
            assert_eq!(
                final_response.content.as_ref().unwrap().parts,
                [Part::Text { text: "Final".into() }]
            );
        }
    }

    #[tokio::test]
    async fn explicit_empty_correction_clears_prior_text() {
        for compatible in [false, true] {
            let results = responses(
                &[delta("msg", 0, 0, "Draft"), done("msg", 0, 0, ""), completion(json!([]))],
                compatible,
                "",
            )
            .await;
            let final_response = results.last().unwrap();
            assert_eq!(
                final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                true
            );
            assert!(final_response.content.as_ref().unwrap().parts.is_empty());
        }
    }

    #[tokio::test]
    async fn done_correction_survives_omitted_completed_output() {
        for compatible in [false, true] {
            let results = responses(
                &[delta("msg", 0, 0, "Draft"), done("msg", 0, 0, "Final"), completion(json!([]))],
                compatible,
                "",
            )
            .await;
            let final_response = results.last().unwrap();
            assert_eq!(
                final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                true
            );
            assert_eq!(
                final_response.content.as_ref().unwrap().parts,
                [Part::Text { text: "Final".into() }]
            );
        }
    }

    #[tokio::test]
    async fn explicit_completed_empty_text_supersedes_done() {
        for compatible in [false, true] {
            let results = responses(
                &[
                    done("msg", 0, 0, "Draft"),
                    completion(json!([message("msg", json!([text("")]))])),
                ],
                compatible,
                "",
            )
            .await;
            let final_response = results.last().unwrap();
            assert_eq!(
                final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                true
            );
            assert!(final_response.content.as_ref().unwrap().parts.is_empty());
        }
    }

    #[tokio::test]
    async fn unfinished_deltas_are_not_final_answers() {
        for compatible in [false, true] {
            let results =
                responses(&[delta("msg", 0, 0, "Draft"), completion(json!([]))], compatible, "")
                    .await;
            let final_response = results.last().unwrap();
            assert_eq!(
                final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                true
            );
            assert!(final_response.content.as_ref().unwrap().parts.is_empty());
        }
    }

    #[tokio::test]
    async fn finalized_parts_drop_unfinished_deltas() {
        for compatible in [false, true] {
            let results = responses(
                &[
                    delta("msg", 0, 0, "Kept"),
                    done("msg", 0, 0, "Kept"),
                    delta("msg", 0, 1, "Draft"),
                    completion(json!([])),
                ],
                compatible,
                "",
            )
            .await;
            let final_response = results.last().unwrap();
            assert_eq!(
                final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                true
            );
            assert_eq!(
                final_response.content.as_ref().unwrap().parts,
                [Part::Text { text: "Kept".into() }]
            );
        }
    }

    #[tokio::test]
    async fn empty_content_clears_prior_parts() {
        for compatible in [false, true] {
            for initial in [delta("msg", 0, 0, "Draft"), done("msg", 0, 0, "Draft")] {
                let results = responses(
                    &[initial, completion(json!([message("msg", json!([]))]))],
                    compatible,
                    "",
                )
                .await;
                let final_response = results.last().unwrap();
                assert_eq!(
                    final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                    true
                );
                assert!(final_response.content.as_ref().unwrap().parts.is_empty());
            }
        }
    }

    #[tokio::test]
    async fn missing_final_items_clear_unfinished_text() {
        for compatible in [false, true] {
            let results = responses(
                &[
                    delta("msg", 0, 0, "Draft"),
                    completion(json!([{"type":"reasoning", "id":"rs", "summary":[
                        {"type":"summary_text", "text":"Thinking"}
                    ]}])),
                ],
                compatible,
                "",
            )
            .await;
            let final_response = results.last().unwrap();
            assert_eq!(
                final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                true
            );
            assert_eq!(
                final_response.content.as_ref().unwrap().parts,
                [Part::Thinking { thinking: "Thinking".into(), signature: None }]
            );
        }
    }

    #[tokio::test]
    async fn recovered_parts_keep_authoritative_order() {
        for compatible in [false, true] {
            for (initial, output) in [
                (
                    delta("msg", 0, 1, "Second"),
                    json!([message("msg", json!([text("First"), text("Second")]))]),
                ),
                (
                    delta("second", 1, 0, "Second"),
                    json!([
                        message("first", json!([text("First")])),
                        message("second", json!([text("Second")]))
                    ]),
                ),
            ] {
                let results = responses(&[initial, completion(output)], compatible, "").await;
                let final_response = results.last().unwrap();
                assert_eq!(
                    final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                    true
                );
                assert_eq!(
                    final_response.content.as_ref().unwrap().parts,
                    [Part::Text { text: "First".into() }, Part::Text { text: "Second".into() }]
                );
            }
        }
    }

    #[tokio::test]
    async fn late_suffix_keeps_authoritative_order() {
        for compatible in [false, true] {
            let results = responses(
                &[
                    delta("msg", 0, 0, "F"),
                    delta("msg", 0, 1, "Second"),
                    done("msg", 0, 0, "First"),
                    completion(json!([message("msg", json!([text("First"), text("Second")]))])),
                ],
                compatible,
                "",
            )
            .await;
            let final_response = results.last().unwrap();
            assert_eq!(
                final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                true
            );
            assert_eq!(
                final_response.content.as_ref().unwrap().parts,
                [Part::Text { text: "First".into() }, Part::Text { text: "Second".into() }]
            );
        }
    }

    #[tokio::test]
    async fn delta_order_matches_final_content() {
        for compatible in [false, true] {
            for (mut events, output) in [
                (
                    vec![
                        delta("msg", 0, 0, "F"),
                        delta("msg", 0, 1, "Second"),
                        delta("msg", 0, 0, "irst"),
                    ],
                    json!([message("msg", json!([text("First"), text("Second")]))]),
                ),
                (
                    vec![delta("second", 1, 0, "Second"), delta("first", 0, 0, "First")],
                    json!([
                        message("first", json!([text("First")])),
                        message("second", json!([text("Second")]))
                    ]),
                ),
            ] {
                events.push(completion(output));
                let results = responses(&events, compatible, "").await;
                let final_response = results.last().unwrap();
                assert_eq!(
                    final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                    true
                );
                assert_eq!(
                    final_response.content.as_ref().unwrap().parts,
                    [Part::Text { text: "First".into() }, Part::Text { text: "Second".into() }]
                );
            }
        }
    }

    #[tokio::test]
    async fn compatible_missing_ids_replace_fragments() {
        let results = responses(
            &[
                delta("msg", 0, 0, "Answer"),
                completion(
                    json!([{"type":"message", "role":"assistant", "content":[text("Answer")]}]),
                ),
            ],
            true,
            "",
        )
        .await;
        let final_response = results.last().unwrap();
        assert_eq!(final_response.provider_metadata.as_ref().unwrap()["content_complete"], true);
        assert_eq!(
            final_response.content.as_ref().unwrap().parts,
            [Part::Text { text: "Answer".into() }]
        );
    }

    #[tokio::test]
    async fn final_suffix_keeps_citation_offsets() {
        for compatible in [false, true] {
            let results = responses(&[
                delta("msg", 0, 0, "First"),
                completion(json!([message("msg", json!([{
                    "type":"output_text", "text":"First tail", "annotations":[{
                        "type":"url_citation", "url":"https://example.test", "title":"Example",
                        "start_index":6, "end_index":10
                    }]
                }]))])),
            ], compatible, "").await;
            assert_eq!(text_parts(&results), ["First", " tail"]);
            let citation =
                &results.last().unwrap().citation_metadata.as_ref().unwrap().citation_sources[0];
            assert_eq!(citation.start_index, Some(6));
            assert_eq!(citation.end_index, Some(10));
        }
    }

    #[tokio::test]
    async fn incomplete_text_is_recovered() {
        for compatible in [false, true] {
            let mut final_event =
                completion(json!([message("msg", json!([text("Partial answer")]))]));
            final_event["type"] = "response.incomplete".into();
            final_event["response"]["status"] = "incomplete".into();
            final_event["response"]["incomplete_details"] = json!({"reason":"max_output_tokens"});
            let results = responses(&[final_event], compatible, "").await;
            assert_eq!(text_parts(&results), ["Partial answer"]);
            assert!(results[0].turn_complete);
            assert_eq!(results[0].finish_reason, Some(adk_core::FinishReason::MaxTokens));
        }
    }
}

mod refusal_recovery {
    use super::*;

    #[tokio::test]
    async fn completed_refusal_is_visible() {
        for compatible in [false, true] {
            let results = responses(
                &[completion(json!([message(
                    "msg",
                    json!([
                        {"type":"refusal", "refusal":"I cannot help with that"}
                    ])
                )]))],
                compatible,
                "",
            )
            .await;
            assert_eq!(text_parts(&results), ["I cannot help with that"]);
            assert!(!results[0].partial);
        }
    }

    #[tokio::test]
    async fn repeated_refusal_payloads_emit_only_suffixes() {
        for compatible in [false, true] {
            let refusal = json!({"type":"refusal", "refusal":"I cannot help"});
            let item = message("msg", json!([refusal]));
            let results = responses(&[
                json!({"type":"response.refusal.delta", "sequence_number":1,
                    "item_id":"msg", "output_index":0, "content_index":0, "delta":"I cannot"}),
                json!({"type":"response.refusal.done", "sequence_number":2,
                    "item_id":"msg", "output_index":0, "content_index":0, "refusal":"I cannot help"}),
                json!({"type":"response.content_part.done", "sequence_number":3,
                    "item_id":"msg", "output_index":0, "content_index":0, "part":refusal}),
                json!({"type":"response.output_item.done", "sequence_number":4,
                    "output_index":0, "item":item}),
                completion(json!([item])),
            ], compatible, "").await;
            assert_eq!(text_parts(&results), ["I cannot", " help"]);
        }
    }

    #[tokio::test]
    async fn following_text_keeps_citation_offsets() {
        for compatible in [false, true] {
            let results = responses(
                &[completion(json!([message(
                    "msg",
                    json!([
                        {"type":"refusal", "refusal":"Cannot"},
                        {"type":"output_text", "text":"Details", "annotations":[{
                            "type":"url_citation", "url":"https://example.test", "title":"Example",
                            "start_index":0, "end_index":7
                        }]}
                    ])
                )]))],
                compatible,
                "",
            )
            .await;
            assert_eq!(text_parts(&results), ["Cannot", "Details"]);
            let citation = &results[0].citation_metadata.as_ref().unwrap().citation_sources[0];
            assert_eq!(citation.start_index, Some(6));
            assert_eq!(citation.end_index, Some(13));
        }
    }
}

mod reasoning_recovery {
    use super::*;

    #[tokio::test]
    async fn summaries_emit_only_unseen_text() {
        for compatible in [false, true] {
            let item = json!({"type":"reasoning", "id":"rs", "status":"completed", "summary":[
                {"type":"summary_text", "text":"Thinking"},
                {"type":"summary_text", "text":"More"}
            ]});
            let results = responses(
                &[
                    json!({"type":"response.reasoning_summary_text.delta", "sequence_number":1,
                    "item_id":"rs", "output_index":0, "summary_index":0, "delta":"Think"}),
                    json!({"type":"response.reasoning_summary_text.done", "sequence_number":2,
                    "item_id":"rs", "output_index":0, "summary_index":0, "text":"Thinking"}),
                    json!({"type":"response.reasoning_summary_part.done", "sequence_number":3,
                    "item_id":"rs", "output_index":0, "summary_index":0,
                    "part":{"type":"summary_text", "text":"Thinking"}}),
                    json!({"type":"response.output_item.done", "sequence_number":4,
                    "output_index":0, "item":item}),
                    completion(json!([item, message("msg", json!([text("Answer")]))])),
                ],
                compatible,
                "",
            )
            .await;
            let thinking = results
                .iter()
                .filter_map(|response| response.content.as_ref())
                .flat_map(|content| &content.parts)
                .filter_map(|part| {
                    if let Part::Thinking { thinking, .. } = part {
                        Some(thinking.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(thinking, ["Think", "ing", "More"]);
            assert_eq!(text_parts(&results), ["Answer"]);
        }
    }

    #[tokio::test]
    async fn completed_summary_without_deltas() {
        for compatible in [false, true] {
            let results = responses(&[completion(json!([
                {"type":"reasoning", "id":"rs", "summary":[{"type":"summary_text", "text":"Thinking"}]},
                message("msg", json!([text("Answer")]))
            ]))], compatible, "").await;
            assert_eq!(
                results[0].content.as_ref().unwrap().parts,
                [
                    Part::Thinking { thinking: "Thinking".into(), signature: None },
                    Part::Text { text: "Answer".into() }
                ]
            );
        }
    }
}

mod stream_activity {
    use super::*;

    #[tokio::test]
    async fn keeps_empty_tool_activity() {
        for compatible in [false, true] {
            let item = json!({"type":"function_call", "id":"fc", "call_id":"call",
                "name":"read_file", "arguments":r#"{"path":"first.txt"}"#, "status":"completed"});
            let results = try_responses(
                &[
                    json!({"type":"response.output_item.added", "sequence_number":1,
                    "output_index":0, "item":item}),
                    json!({"type":"response.function_call_arguments.delta", "sequence_number":2,
                    "output_index":0, "item_id":"fc", "delta":r#"{"path":"first.txt"}"#}),
                    completion(json!([item])),
                ],
                compatible,
                "",
            )
            .await
            .unwrap();
            assert_eq!(results.len(), 3);
            assert!(empty_activity(&results[0]));
            assert!(empty_activity(&results[1]));
            assert!(results[2].turn_complete);
            assert_eq!(
                results[2].content.as_ref().unwrap().parts,
                [Part::FunctionCall {
                    id: Some("call".into()),
                    name: "read_file".into(),
                    args: json!({"path":"first.txt"}),
                    thought_signature: None
                }]
            );
        }
    }
}

mod commentary_recovery {
    use super::*;

    #[tokio::test]
    async fn omitted_output_groups_finalized_parts() {
        for compatible in [false, true] {
            let results = responses(&[
                json!({"type":"response.output_item.added", "sequence_number":0, "output_index":0,
                    "item":{"type":"message", "id":"msg", "role":"assistant", "status":"in_progress",
                        "phase":"commentary", "content":[]}}),
                delta("msg", 0, 0, "Check"), done("msg", 0, 0, "Checking"),
                done("msg", 0, 1, "More"), completion(json!([])),
            ], compatible, "").await;
            let final_response = results.last().unwrap();
            assert_eq!(
                final_response.provider_metadata.as_ref().unwrap()["content_complete"],
                true
            );
            let parts = &final_response.content.as_ref().unwrap().parts;
            assert_eq!(parts.len(), 1);
            let Part::ServerToolCall { server_tool_call } = &parts[0] else {
                panic!("commentary remains a native message");
            };
            assert_eq!(
                server_tool_call,
                &json!({
                    "type":"message", "id":"msg", "role":"assistant", "status":"completed", "phase":"commentary",
                    "content":[text("Checking"), text("More")]
                })
            );
        }
    }
}
