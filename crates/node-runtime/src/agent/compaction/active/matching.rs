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
            return Err(invalid(
                "model context does not match the persisted summary",
            ));
        }
        request
            .contents
            .splice(1..self.index, [self.summary.clone()]);
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
    let original = event.content()?;
    let metadata = event.llm_response.provider_metadata.as_ref();
    // Runner requires ownership when replaying prior invocations; LlmAgent's
    // current invocation appends provider parts before that replay boundary.
    let mut content = if event.invocation_id == invocation
        || adk_core::history::provider_origin(metadata).is_some()
    {
        adk_core::history::with_provider_parts(original, metadata)
    } else {
        original.clone()
    };
    content.role = match (event.author.as_str(), original.role.as_str()) {
        ("user", _) => "user",
        (_, "function" | "tool") => original.role.as_str(),
        _ => "model",
    }
    .into();
    Some(content)
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

fn same(actual: &Content, stored: &Content) -> bool {
    let mut actual = actual.clone();
    let mut stored = stored.clone();
    actual.strip_generated_call_ids();
    stored.strip_generated_call_ids();
    actual.role == stored.role && actual.parts == stored.parts
}

fn matches(actual: &[Content], stored: &[Content]) -> bool {
    matches_refs(actual, &stored.iter().collect::<Vec<_>>())
}

fn matches_refs(actual: &[Content], stored: &[&Content]) -> bool {
    if actual.len() < stored.len() {
        return false;
    }
    let mut index = 0;
    while index < stored.len() {
        if response(stored[index]).is_none() {
            if !same(&actual[index], stored[index]) {
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
        if stored[index..end]
            .iter()
            .any(|content| response(content).is_some_and(|(id, _)| id.starts_with("adk-call-")))
        {
            if !generated(&actual[index..end], &stored[index..end], &stored[..index]) {
                return false;
            }
            index = end;
            continue;
        }
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

// ADK strips its synthetic IDs before this callback. Persisted results retain
// completion order, while the live loop assembles them in call order. Accept
// those two projections only, comparing complete parts and preserving provider IDs.
fn generated(actual: &[Content], stored: &[&Content], prefix: &[&Content]) -> bool {
    let mut identities = std::collections::HashSet::new();
    if stored
        .iter()
        .any(|content| !identities.insert(response(content).unwrap()))
    {
        return false;
    }
    let ordered = |expected: &[&Content]| actual.iter().zip(expected).all(|(a, b)| same(a, b));
    if ordered(stored) {
        return true;
    }
    let calls = prefix
        .iter()
        .flat_map(|content| &content.parts)
        .filter_map(|part| match part.provider_part() {
            Part::FunctionCall {
                id: Some(id), name, ..
            } => Some((id.as_str(), name.as_str())),
            _ => None,
        })
        .enumerate()
        .map(|(index, key)| (key, index))
        .collect::<std::collections::HashMap<_, _>>();
    let mut sorted = stored.to_vec();
    if sorted
        .iter()
        .any(|content| !calls.contains_key(&response(content).unwrap()))
    {
        return false;
    }
    sorted.sort_by_key(|content| calls[&response(content).unwrap()]);
    ordered(&sorted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adk_core::FunctionResponseData;

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
            content.strip_generated_call_ids();
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
        actual
            .iter_mut()
            .for_each(Content::strip_generated_call_ids);
        assert!(matches(&actual, &stored));
        actual.swap(1, 2);
        assert!(!matches(&actual, &stored));
        let mut replay = stored.clone();
        replay
            .iter_mut()
            .for_each(Content::strip_generated_call_ids);
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
