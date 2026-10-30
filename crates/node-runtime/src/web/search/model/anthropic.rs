use super::*;
use serde_json::json;

pub(super) async fn execute(
    client: &Client,
    prepared: &mut Prepared,
    run: &Run<'_>,
) -> Result<Parsed, Failure> {
    let mut content = Vec::new();
    let mut total = None;
    for continuation in 0..=3 {
        let mut value = send(client, prepared, run).await?;
        check(&value)?;
        accumulate(&mut total, usage(&value));
        if value.get("stop_reason").and_then(Value::as_str) == Some("pause_turn") {
            if continuation == 3 {
                return Err(invalid("model search continuation limit reached"));
            }
            let blocks = value
                .get("content")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("paused model search has no assistant content"))?;
            content.extend(blocks.iter().cloned());
            prepared.body["messages"]
                .as_array_mut()
                .ok_or_else(|| invalid("model search messages are invalid"))?
                .push(json!({"role":"assistant","content":blocks}));
            continue;
        }
        if !content.is_empty() {
            let blocks = value
                .get("content")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("model search response has no content"))?;
            content.extend(blocks.iter().cloned());
            value["content"] = Value::Array(content);
        }
        let mut result = parse(&value)?;
        result.usage = total;
        return Ok(result);
    }
    Err(invalid("model search continuation limit reached"))
}

pub(super) fn body(model: &str, input: &search::Request) -> Value {
    let mut tool = json!({"type":"web_search_20250305", "name":"web_search", "max_uses":3});
    // The Messages API accepts either allow or block domains, never both.
    if !input.include_domains.is_empty() {
        tool["allowed_domains"] = json!(input.include_domains);
    } else if !input.exclude_domains.is_empty() {
        tool["blocked_domains"] = json!(input.exclude_domains);
    }
    json!({
        "model":model,
        "max_tokens":4096,
        "messages":[{"role":"user","content":filters::query(input)}],
        "tools":[tool],
        "tool_choice":{"type":"auto"}
    })
}

pub(super) fn parse(value: &Value) -> Result<Parsed, Failure> {
    check(value)?;
    if value.get("stop_reason").and_then(Value::as_str) != Some("end_turn") {
        return Err(invalid("model search response did not complete"));
    }
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("model search response has no content"))?;
    let mut results = Vec::new();
    let mut parts = Vec::new();
    let mut searched = false;
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("web_search_tool_result") => {
                let result = &block["content"];
                if result.get("type").and_then(Value::as_str)
                    == Some("web_search_tool_result_error")
                {
                    continue;
                }
                let entries = result
                    .as_array()
                    .ok_or_else(|| invalid("model search result list is invalid"))?;
                searched = true;
                for entry in entries {
                    if entry.get("type").and_then(Value::as_str) != Some("web_search_result") {
                        continue;
                    }
                    let url = entry
                        .get("url")
                        .and_then(Value::as_str)
                        .ok_or_else(|| invalid("model search result URL is missing"))?;
                    push(
                        &mut results,
                        url,
                        entry.get("title").and_then(Value::as_str),
                        None,
                    )?;
                }
            }
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    parts.push(text);
                }
                if let Some(citations) = block.get("citations").and_then(Value::as_array) {
                    for citation in citations {
                        if citation.get("type").and_then(Value::as_str)
                            != Some("web_search_result_location")
                        {
                            continue;
                        }
                        let url = citation
                            .get("url")
                            .and_then(Value::as_str)
                            .ok_or_else(|| invalid("model search citation URL is missing"))?;
                        push(
                            &mut results,
                            url,
                            citation.get("title").and_then(Value::as_str),
                            citation.get("cited_text").and_then(Value::as_str),
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    if !searched {
        if let Some(error) = content.iter().find_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
                && block["content"].get("type").and_then(Value::as_str)
                    == Some("web_search_tool_result_error"))
            .then_some(&block["content"])
        }) {
            return Err(network::upstream(error));
        }
        return Err(invalid("model returned no web search metadata"));
    }
    Ok(Parsed {
        results,
        summary: summary(parts),
        usage: usage(value),
    })
}

fn check(value: &Value) -> Result<(), Failure> {
    if value.get("type").and_then(Value::as_str) == Some("error") {
        return Err(network::upstream(value.get("error").unwrap_or(value)));
    }
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        for block in content {
            if block.get("type").and_then(Value::as_str) == Some("web_search_tool_result")
                && block["content"].get("type").and_then(Value::as_str)
                    == Some("web_search_tool_result_error")
                && block["content"].get("error_code").and_then(Value::as_str)
                    != Some("max_uses_exceeded")
            {
                return Err(network::upstream(&block["content"]));
            }
        }
    }
    Ok(())
}

pub(super) fn usage(value: &Value) -> Option<Usage> {
    value
        .get("usage")
        .filter(|value| value.is_object())
        .map(|usage| {
            let read = tokens(usage, "/cache_read_input_tokens");
            let write = usage
                .get("cache_creation_input_tokens")
                .and_then(Value::as_u64);
            Usage {
                input: Some(
                    tokens(usage, "/input_tokens")
                        .saturating_add(read)
                        .saturating_add(write.unwrap_or_default()),
                ),
                output: Some(tokens(usage, "/output_tokens")),
                cache_read: Some(read),
                cache_write: write,
                reasoning: Some(0),
                ..Default::default()
            }
        })
}

fn accumulate(total: &mut Option<Usage>, reported: Option<Usage>) {
    let Some(reported) = reported else {
        return;
    };
    if let Some(total) = total {
        for (count, additional) in [
            (&mut total.input, reported.input),
            (&mut total.output, reported.output),
            (&mut total.total, reported.total),
            (&mut total.cache_read, reported.cache_read),
            (&mut total.cache_write, reported.cache_write),
            (&mut total.reasoning, reported.reasoning),
        ] {
            if let Some(additional) = additional {
                *count = Some(count.unwrap_or_default().saturating_add(additional));
            }
        }
    } else {
        *total = Some(reported);
    }
}
