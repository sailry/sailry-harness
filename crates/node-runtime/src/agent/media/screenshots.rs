//! Omit configured MCP images only in the model projection, after compaction.
use adk_core::{BeforeModelCallback, BeforeModelResult, LlmRequest, Part};

const BATCH: usize = 5;

pub(in crate::agent) fn callback(servers: Vec<(Vec<String>, usize)>) -> BeforeModelCallback {
    Box::new(move |_, mut request| {
        for (aliases, keep) in &servers {
            omit(&mut request, aliases, *keep);
        }
        Box::pin(async move { Ok(BeforeModelResult::Continue(request)) })
    })
}

fn omit(request: &mut LlmRequest, aliases: &[String], keep: usize) {
    let image = |data: &adk_core::InlineDataPart| data.mime_type.starts_with("image/");
    let mut responses: Vec<_> = request
        .contents
        .iter_mut()
        .flat_map(|content| content.parts.iter_mut())
        .filter_map(|part| match part {
            Part::FunctionResponse {
                function_response, ..
            } if aliases.contains(&function_response.name) => Some(function_response),
            _ => None,
        })
        .collect();
    let total: usize = responses
        .iter()
        .map(|response| {
            response
                .inline_data
                .iter()
                .filter(|data| image(data))
                .count()
        })
        .sum();
    let mut remaining = total.saturating_sub(keep) / BATCH * BATCH;
    if remaining == 0 {
        return;
    }
    for response in &mut responses {
        if remaining == 0 {
            break;
        }
        let before = response.inline_data.len();
        response.inline_data.retain(|data| {
            if remaining > 0 && image(data) {
                remaining -= 1;
                false
            } else {
                true
            }
        });
        let omitted = before - response.inline_data.len();
        if omitted > 0
            && let Some(object) = response.response.as_object_mut()
        {
            let previous = object
                .get("omitted_screenshots")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            object.insert(
                "omitted_screenshots".into(),
                serde_json::json!(previous.saturating_add(omitted as u64)),
            );
        }
    }
    request.previous_response_id = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use adk_core::{Content, FunctionResponseData, InlineDataPart};
    use serde_json::json;

    fn image() -> InlineDataPart {
        InlineDataPart {
            mime_type: "image/png".into(),
            data: vec![1],
            uri: None,
            annotations: None,
        }
    }

    fn response(name: &str, count: usize) -> Part {
        let mut response =
            FunctionResponseData::new(name, json!({"isError":false,"omitted_screenshots":2}));
        response.inline_data = (0..count).map(|_| image()).collect();
        Part::FunctionResponse {
            function_response: response,
            id: None,
            annotations: None,
        }
    }

    #[test]
    fn preserves_batch_boundaries() {
        for keep in [0, 3, 8] {
            for count in 0..20 {
                let mut content = Content::new("user");
                content.parts = vec![
                    response("configured", count),
                    response("other", 2),
                    Part::inline_data("image/png", vec![1]),
                ];
                let mut request = LlmRequest::new("model", vec![content]);
                request.previous_response_id = Some("previous".into());
                let original = request.clone();
                omit(&mut request, &["configured".into()], keep);
                let removed = count.saturating_sub(keep) / 5 * 5;
                let Part::FunctionResponse {
                    function_response, ..
                } = &request.contents[0].parts[0]
                else {
                    unreachable!()
                };
                assert_eq!(function_response.inline_data.len(), count - removed);
                assert_eq!(
                    function_response.response["omitted_screenshots"],
                    2 + removed
                );
                assert_eq!(
                    &request.contents[0].parts[1..],
                    &original.contents[0].parts[1..]
                );
                assert_eq!(request.previous_response_id.is_none(), removed > 0);
                if removed == 0 {
                    assert_eq!(
                        serde_json::to_value(&request).unwrap(),
                        serde_json::to_value(&original).unwrap()
                    );
                }
            }
        }
    }
}
