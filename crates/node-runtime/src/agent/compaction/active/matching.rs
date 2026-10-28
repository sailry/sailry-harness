//! Compare only one tool batch by call identity; canonical history is unchanged.
use super::*;

pub(super) struct Checkpoint {
    index: usize,
    retained: Vec<Content>,
    summary: Content,
}

impl Checkpoint {
    pub(super) fn new(request: &LlmRequest, index: usize, summary: Content, offset: usize) -> Self {
        Self {
            index: index + offset,
            retained: request.contents[index..].to_vec(),
            summary,
        }
    }

    pub(super) fn offset(&self) -> usize {
        self.index - 2
    }

    pub(super) fn apply(
        &self,
        request: &mut LlmRequest,
        preamble: &Content,
    ) -> adk_core::Result<()> {
        if !preamble_matches(request, preamble)
            || !request
                .contents
                .get(self.index..)
                .is_some_and(|contents| matches(contents, &self.retained))
        {
            return Err(invalid_at(
                Stage::History,
                Reason::HistoryMismatch,
                "model context does not match the persisted summary",
            ));
        }
        request
            .contents
            .splice(1..self.index, [self.summary.clone()]);
        model::history::clear_prepared(request);
        request.previous_response_id = None;
        Ok(())
    }
}

fn preamble_matches(request: &LlmRequest, preamble: &Content) -> bool {
    request.contents.first().is_some_and(|content| {
        content.role == preamble.role && content.parts.starts_with(&preamble.parts)
    })
}

pub(super) fn tail(
    request: &LlmRequest,
    events: &[Event],
    preamble: &Content,
    invocation: &str,
) -> Option<usize> {
    if !preamble_matches(request, preamble) {
        return None;
    }
    // Match the same projection LlmAgent passes to before_model, before the
    // provider boundary unwraps ownership. Never discard opaque reasoning.
    let retained = events
        .iter()
        .filter_map(|event| project(event, invocation))
        .collect::<Vec<_>>();
    if retained.is_empty() {
        return None;
    }
    let index = request.contents.len().checked_sub(retained.len())?;
    (index > 0 && matches(&request.contents[index..], &retained)).then_some(index)
}

fn project(event: &Event, invocation: &str) -> Option<Content> {
    super::super::history::project(event, invocation)
}

pub(super) fn rebuild(
    request: &mut LlmRequest,
    events: &[Event],
    preamble: &Content,
    invocation: &str,
) -> adk_core::Result<()> {
    if !preamble_matches(request, preamble) {
        return Err(invalid_at(
            Stage::History,
            Reason::HistoryMismatch,
            "model preamble does not match its captured instructions",
        ));
    }
    let mut contents = events
        .iter()
        .filter_map(|event| project(event, invocation))
        .collect::<Vec<_>>();
    restore_order(&mut contents, &request.contents[1..]);
    request.contents.truncate(1);
    model::history::clear_prepared(request);
    request.contents.extend(contents);
    request.previous_response_id = None;
    Ok(())
}

fn response(content: &Content) -> Option<(&str, &str)> {
    match content.parts.first()? {
        Part::FunctionResponse {
            id: Some(id),
            function_response,
            ..
        } => Some((id, &function_response.name)),
        _ => None,
    }
}

fn restore_order(contents: &mut [Content], live: &[Content]) {
    let batches = live
        .chunk_by(|left, right| response(left).is_some() && response(right).is_some())
        .filter(|batch| response(&batch[0]).is_some())
        .collect::<Vec<_>>();
    let mut available = batches.len();
    // A rebuilt summary can remove an old prefix. Pair its retained batches
    // from the end, consuming each live batch at most once.
    for batch in contents
        .chunk_by_mut(|left, right| response(left).is_some() && response(right).is_some())
        .rev()
    {
        if response(&batch[0]).is_none() {
            continue;
        }
        let paired = (0..available).rev().find_map(|index| {
            let candidate = batches[index];
            if candidate.len() != batch.len() {
                return None;
            }
            let positions = candidate
                .iter()
                .enumerate()
                .map(|(index, content)| (response(content).unwrap(), index))
                .collect::<std::collections::HashMap<_, _>>();
            if positions.len() != candidate.len() {
                return None;
            }
            let mut indices = batch
                .iter()
                .map(|content| positions.get(&response(content)?).copied())
                .collect::<Option<Vec<_>>>()?;
            indices.sort_unstable();
            indices
                .iter()
                .copied()
                .eq(0..batch.len())
                .then_some((index, positions))
        });
        let Some((index, positions)) = paired else {
            continue;
        };
        available = index;
        batch.sort_by_key(|content| positions[&response(content).unwrap()]);
    }
}

fn same(actual: &Content, stored: &Content) -> bool {
    actual.role == stored.role && actual.parts == stored.parts
}

fn matches(actual: &[Content], stored: &[Content]) -> bool {
    if actual.len() < stored.len() {
        return false;
    }
    let mut index = 0;
    while index < stored.len() {
        if response(&stored[index]).is_none() {
            if !same(&actual[index], &stored[index]) {
                return false;
            }
            index += 1;
            continue;
        }
        // Adjacent response contents belong to this batch. A new model call or
        // any ordinary content ends it; never reorder across that boundary.
        let end = index
            + stored[index..]
                .iter()
                .take_while(|content| response(content).is_some())
                .count();
        let mut available = std::collections::HashMap::new();
        for content in &actual[index..end] {
            let Some(key) = response(content) else {
                return false;
            };
            if available.insert(key, content).is_some() {
                return false;
            }
        }
        for expected in &stored[index..end] {
            let key = response(expected).unwrap();
            if !available
                .remove(&key)
                .is_some_and(|actual| same(actual, expected))
            {
                return false;
            }
        }
        index = end;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use adk_core::FunctionResponseData;

    #[test]
    fn rebuilds_the_same_saved_window() {
        let preamble = Content::new("system").with_text("Captured instructions");
        let handoff = Content::new("user").with_text("Saved handoff");
        let mut live = Event::new("summary");
        live.author = "system".into();
        live.actions.compaction = Some(EventCompaction {
            start_timestamp: live.timestamp,
            end_timestamp: live.timestamp,
            compacted_content: handoff.clone(),
        });
        let mut loaded = live.clone();
        loaded.actions.compaction = None;
        loaded.author = "user".into();
        loaded.set_content(handoff);
        let mut current = Event::new("current");
        current.author = "user".into();
        current.set_content(Content::new("user").with_text("Latest request"));
        let mut request = LlmRequest::new(
            "fixture",
            vec![
                preamble.clone(),
                Content::new("user").with_text("Stale original prefix"),
            ],
        );
        request.previous_response_id = Some("stale-response".into());
        rebuild(&mut request, &[live, current.clone()], &preamble, "current").unwrap();
        let live = request.contents.clone();
        rebuild(&mut request, &[loaded, current], &preamble, "current").unwrap();
        assert_eq!(request.contents.len(), live.len());
        for (loaded, live) in request.contents.iter().zip(&live) {
            assert_eq!(loaded.role, live.role);
            assert_eq!(loaded.parts, live.parts);
        }
        assert_eq!(request.contents.len(), 3);
        assert_eq!(request.contents[1].role, "user");
        assert_eq!(request.contents[2].parts[0].text(), Some("Latest request"));
        assert!(request.previous_response_id.is_none());
    }

    fn result(id: &str, text: &str) -> Content {
        Content {
            role: "user".into(),
            parts: vec![Part::FunctionResponse {
                id: Some(id.into()),
                function_response: FunctionResponseData::new("read_file", json!({"text":text})),
                annotations: None,
            }],
        }
    }

    #[test]
    fn rebuilds_raw_results_in_live_order() {
        let preamble = Content::new("system").with_text("Captured instructions");
        let calls = |ids: &[&str]| Content {
            role: "model".into(),
            parts: ids
                .iter()
                .map(|id| Part::FunctionCall {
                    id: Some((*id).into()),
                    name: "read_file".into(),
                    args: json!({"path":id}),
                    thought_signature: None,
                })
                .collect(),
        };
        let output = |id: &str| {
            let mut content = result(id, &format!("Canonical {id}"));
            content.role = "function".into();
            content
        };
        let mut request = LlmRequest::new(
            "fixture",
            vec![
                preamble.clone(),
                Content::new("user").with_text("Read the independent files"),
                calls(&["first", "second"]),
                output("first"),
                output("second"),
                calls(&["third"]),
                output("third"),
            ],
        );
        let expected = serde_json::to_vec(&request.contents).unwrap();
        let mut events = request.contents[1..]
            .iter()
            .map(|content| {
                let mut event = Event::new("current");
                event.author = if content.role == "user" {
                    "user"
                } else {
                    "assistant"
                }
                .into();
                event.set_content(content.clone());
                event
            })
            .collect::<Vec<_>>();
        events.swap(2, 3);
        let canonical = serde_json::to_vec(&events).unwrap();
        if let Part::FunctionResponse {
            function_response, ..
        } = &mut request.contents[3].parts[0]
        {
            function_response.response = json!({"truncated":true});
        }
        rebuild(&mut request, &events, &preamble, "current").unwrap();
        assert_eq!(serde_json::to_vec(&request.contents).unwrap(), expected);
        assert_eq!(serde_json::to_vec(&events).unwrap(), canonical);

        // Equal identities across a model boundary are not one response batch.
        request.contents.swap(4, 6);
        rebuild(&mut request, &events, &preamble, "current").unwrap();
        assert_eq!(response(&request.contents[3]).unwrap().0, "second");
        assert_eq!(response(&request.contents[4]).unwrap().0, "first");
        assert_eq!(response(&request.contents[6]).unwrap().0, "third");
    }

    #[test]
    fn rebuilds_reused_ids_in_separate_batches() {
        let preamble = Content::new("system").with_text("Captured instructions");
        let calls = |round, ids: &[&str]| Content {
            role: "model".into(),
            parts: ids
                .iter()
                .map(|id| Part::FunctionCall {
                    id: Some((*id).into()),
                    name: "read_file".into(),
                    args: json!({"path":format!("{round}-{id}")}),
                    thought_signature: None,
                })
                .collect(),
        };
        let output = |round, id: &str| {
            let mut content = result(id, &format!("Canonical {round}-{id}"));
            content.role = "function".into();
            content
        };
        let mut request = LlmRequest::new(
            "fixture",
            vec![
                preamble.clone(),
                Content::new("user").with_text("Read both rounds"),
                calls(0, &["first", "second"]),
                output(0, "first"),
                output(0, "second"),
                calls(1, &["second", "first"]),
                output(1, "second"),
                output(1, "first"),
            ],
        );
        let expected = serde_json::to_vec(&request.contents).unwrap();
        let mut events = request.contents[1..]
            .iter()
            .map(|content| {
                let mut event = Event::new("current");
                event.author = if content.role == "user" {
                    "user"
                } else {
                    "assistant"
                }
                .into();
                event.set_content(content.clone());
                event
            })
            .collect::<Vec<_>>();
        events.swap(2, 3);
        events.swap(5, 6);
        let canonical = serde_json::to_vec(&events).unwrap();
        rebuild(&mut request, &events, &preamble, "current").unwrap();
        assert_eq!(serde_json::to_vec(&request.contents).unwrap(), expected);
        assert_eq!(serde_json::to_vec(&events).unwrap(), canonical);
    }

    #[tokio::test]
    async fn limits_rebuilt_requests_before_generation() {
        let preamble = Content::new("system").with_text("Captured instructions");
        let original = json!({"text":"retained evidence ".repeat(2000)});
        let mut event = Event::new("completed");
        event.set_content(Content {
            role: "function".into(),
            parts: vec![Part::FunctionResponse {
                id: Some("evidence".into()),
                function_response: FunctionResponseData::new("mcp_fixture", original.clone()),
                annotations: None,
            }],
        });
        let mut request = LlmRequest::new(
            "fixture",
            vec![
                preamble.clone(),
                Content::new("user").with_text("Old prefix"),
            ],
        );
        let window = Some(model::history::Window {
            context: 1000,
            output: 100,
        });
        model::history::prepare(&mut request, None)
            .limit(&mut request, window)
            .unwrap();
        assert!(
            request
                .config
                .as_ref()
                .unwrap()
                .extensions
                .contains_key(model::history::PREPARED)
        );
        rebuild(&mut request, &[event], &preamble, "current").unwrap();
        assert!(
            !request
                .config
                .as_ref()
                .unwrap()
                .extensions
                .contains_key(model::history::PREPARED)
        );
        let revision = blake3::hash(original.to_string().as_bytes())
            .to_hex()
            .to_string();
        let stream = model::history::generate(
            request,
            adk_core::ProviderOrigin {
                provider: "responses".into(),
                model: "fixture".into(),
                scope: "fixture".into(),
            },
            window,
            |request| async move {
                let Part::FunctionResponse {
                    function_response, ..
                } = &request.contents[1].parts[0]
                else {
                    panic!("retained result expected")
                };
                assert_eq!(function_response.response["output"]["call"], "evidence");
                assert_eq!(function_response.response["output"]["revision"], revision);
                assert!(estimate::request(&request).unwrap() <= 900);
                assert_eq!(
                    request.contents[0].parts[0].text(),
                    Some("Captured instructions")
                );
                Ok(Box::pin(futures::stream::empty()) as adk_core::LlmResponseStream)
            },
        )
        .await
        .unwrap();
        drop(stream);
    }

    #[test]
    fn matches_parallel_results_by_identity() {
        let preamble = Content::new("system").with_text("Instructions");
        let first = result("read", "Original read");
        let second = result("write", "Original write");
        let mut events = Vec::new();
        for content in [&first, &second] {
            let mut event = Event::new("synthetic");
            event.author = "user".into();
            event.set_content(content.clone());
            events.push(event);
        }
        let mut request = LlmRequest::new(
            "fixture",
            vec![
                preamble.clone(),
                Content::new("user").with_text("Old prefix"),
                second.clone(),
                first.clone(),
            ],
        );
        assert_eq!(tail(&request, &events, &preamble, "current"), Some(2));
        let checkpoint = Checkpoint::new(
            &request,
            2,
            Content::new("model").with_text("Saved summary"),
            0,
        );
        request
            .contents
            .push(Content::new("model").with_text("Later content"));
        request.previous_response_id = Some("old-response".into());
        checkpoint.apply(&mut request, &preamble).unwrap();
        assert_eq!(request.contents[1].parts[0].text(), Some("Saved summary"));
        assert_eq!(request.contents[2].parts, second.parts);
        assert!(request.previous_response_id.is_none());
        assert_eq!(events[0].content().unwrap().parts, first.parts);
        let mut changed = result("read", "Changed payload");
        changed.parts.push(Part::Text {
            text: "Changed".into(),
        });
        request.contents[3] = changed;
        assert!(tail(&request, &events, &preamble, "current").is_none());
        let error = checkpoint.apply(&mut request, &preamble).unwrap_err();
        assert_eq!(
            failure::classify(&error),
            (Stage::History, Reason::HistoryMismatch)
        );
    }

    #[test]
    fn keeps_separate_batches_in_order() {
        let first = result("first", "First evidence");
        let second = result("second", "Second evidence");
        let boundary = Content::new("model").with_text("New batch");
        assert!(!matches(
            &[second.clone(), boundary.clone(), first.clone()],
            &[first, boundary, second]
        ));
        let duplicate = result("same", "Evidence");
        assert!(!matches(
            &[duplicate.clone(), duplicate.clone()],
            &[duplicate.clone(), duplicate]
        ));
    }

    #[test]
    fn preserves_owned_projection_and_generated_ids() {
        let preamble = Content::new("system").with_text("Instructions");
        let mut call = Event::new("invocation");
        call.author = "assistant".into();
        call.set_content(Content {
            role: "assistant".into(),
            parts: vec![
                Part::Thinking {
                    thinking: "Reasoning".into(),
                    signature: Some("signature".into()),
                },
                Part::FunctionCall {
                    name: "read_file".into(),
                    args: json!({"path":"a"}),
                    id: Some("adk-call-generated".into()),
                    thought_signature: Some("call-signature".into()),
                },
            ],
        });
        call.llm_response.provider_metadata = Some(json!({
            "provider_origin":{"provider":"responses","model":"fixture","scope":"owner"},
            "conversation_history_parts":[{"server_tool_call":{"type":"message","phase":"commentary","content":[]}},
                {"server_tool_call":{"type":"reasoning","encrypted_content":"opaque"}}]
        }));
        let mut result = Event::new("invocation");
        result.author = "assistant".into();
        result.set_content(Content {
            role: "function".into(),
            parts: vec![Part::FunctionResponse {
                id: Some("adk-call-generated".into()),
                function_response: FunctionResponseData::new(
                    "read_file",
                    json!({"text":"evidence"}),
                ),
                annotations: None,
            }],
        });
        let events = vec![call, result];
        let canonical = events.clone();
        let mut contents = vec![
            preamble.clone(),
            Content::new("user").with_text("Prior history"),
        ];
        contents.extend(events.iter().map(|event| {
            let mut content = adk_core::history::with_provider_parts(
                event.content().unwrap(),
                event.llm_response.provider_metadata.as_ref(),
            );
            if content.role == "assistant" {
                content.role = "model".into();
            }
            content
        }));
        let mut request = LlmRequest::new("fixture", contents);
        assert_eq!(tail(&request, &events, &preamble, "current"), Some(2));
        assert_eq!(
            serde_json::to_value(&events).unwrap(),
            serde_json::to_value(&canonical).unwrap()
        );
        request.contents[2].parts.remove(0);
        assert_eq!(tail(&request, &events, &preamble, "current"), None);
    }

    #[test]
    fn matches_generated_results_in_call_order() {
        let calls = Content {
            role: "model".into(),
            parts: (1..=3)
                .map(|index| Part::FunctionCall {
                    id: Some(format!("adk-call-{index}")),
                    name: "read_file".into(),
                    args: json!({"path":index}),
                    thought_signature: None,
                })
                .collect(),
        };
        let results = (1..=3)
            .map(|index| result(&format!("adk-call-{index}"), &format!("evidence-{index}")))
            .collect::<Vec<_>>();
        let stored = vec![
            calls.clone(),
            results[2].clone(),
            results[0].clone(),
            results[1].clone(),
        ];
        let mut actual = vec![
            calls,
            results[0].clone(),
            results[1].clone(),
            results[2].clone(),
        ];
        assert!(matches(&actual, &stored));
        actual.swap(1, 2);
        assert!(matches(&actual, &stored));
        let mut replay = stored.clone();
        assert!(matches(&replay, &stored));
        replay[1].parts[0] = result("provider-id", "evidence-3").parts.remove(0);
        assert!(!matches(&replay, &stored));
    }

    #[test]
    fn composes_saved_prefix_offsets() {
        let preamble = Content::new("system").with_text("Instructions");
        let mut original = LlmRequest::new(
            "fixture",
            vec![
                preamble.clone(),
                Content::new("user").with_text("Old first"),
                Content::new("model").with_text("Old second"),
                Content::new("user").with_text("Retained first"),
                Content::new("model").with_text("Retained second"),
                Content::new("user").with_text("Latest task"),
            ],
        );
        let first = Checkpoint::new(
            &original,
            3,
            Content::new("model").with_text("First summary"),
            0,
        );
        let mut projected = original.clone();
        first.apply(&mut projected, &preamble).unwrap();
        let second = Checkpoint::new(
            &projected,
            4,
            Content::new("model").with_text("Second summary"),
            first.offset(),
        );
        second.apply(&mut original, &preamble).unwrap();
        assert_eq!(original.contents.len(), 3);
        assert_eq!(original.contents[1].parts[0].text(), Some("Second summary"));
        assert_eq!(original.contents[2].parts[0].text(), Some("Latest task"));
    }
}

#[cfg(test)]
mod origin;
