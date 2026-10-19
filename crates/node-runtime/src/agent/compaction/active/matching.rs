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

pub(super) fn tail(request: &LlmRequest, events: &[Event], preamble: &Content) -> Option<usize> {
    if !preamble_matches(request, preamble) {
        return None;
    }
    let retained = events.iter().filter_map(Event::content).collect::<Vec<_>>();
    if retained.is_empty() {
        return None;
    }
    let index = request.contents.len().checked_sub(retained.len())?;
    (index > 0 && matches_refs(&request.contents[index..], &retained)).then_some(index)
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
    actual.role == stored.role && actual.parts.starts_with(&stored.parts)
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
            event.set_content(content.clone());
            events.push(event);
        }
        let mut request = LlmRequest::new(
            "fixture",
            vec![
                preamble.clone(),
                Content::new("user").with_text("Old prefix"),
                second.clone().with_text("Provider suffix"),
                first.clone(),
            ],
        );
        assert_eq!(tail(&request, &events, &preamble), Some(2));
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
        assert_eq!(
            request.contents[2].parts,
            second.with_text("Provider suffix").parts
        );
        assert!(request.previous_response_id.is_none());
        assert_eq!(events[0].content().unwrap().parts, first.parts);
        let mut changed = result("read", "Changed payload");
        changed.parts.push(Part::Text {
            text: "Changed".into(),
        });
        request.contents[3] = changed;
        assert!(tail(&request, &events, &preamble).is_none());
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
